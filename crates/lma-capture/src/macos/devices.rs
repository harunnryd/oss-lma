use std::collections::HashSet;

use std::sync::Arc;

use cidre::{
    blocks,
    core_audio::{Device, Obj, PropAddr, PropListenerBlock, PropSelector, System},
    dispatch,
};

use crate::DeviceInfo;

use super::{NativeStreamEvents, SourceKind};

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum DeviceKind {
    SystemOutput,
    Microphone,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeviceSelection {
    Default,
    DeviceId(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeDevice {
    pub id: String,
    pub name: String,
    pub kind: DeviceKind,
    pub is_default: bool,
}

pub trait DeviceProvider {
    fn list(&self) -> Result<Vec<NativeDevice>, String>;
}

#[derive(Clone, Copy)]
pub struct NativeDevices;

impl DeviceProvider for NativeDevices {
    fn list(&self) -> Result<Vec<NativeDevice>, String> {
        let default_input = System::default_input_device().ok();
        let default_output = System::default_output_device().ok();
        let mut devices = Vec::new();
        for device in System::devices().map_err(|error| format!("{error:?}"))? {
            if has_channels(&device, DeviceKind::SystemOutput) {
                devices.push(native_device(
                    device,
                    DeviceKind::SystemOutput,
                    default_output == Some(device),
                )?);
            }
            if has_channels(&device, DeviceKind::Microphone) {
                devices.push(native_device(
                    device,
                    DeviceKind::Microphone,
                    default_input == Some(device),
                )?);
            }
        }
        Ok(devices)
    }
}

fn has_channels(device: &Device, kind: DeviceKind) -> bool {
    let config = match kind {
        DeviceKind::SystemOutput => device.output_stream_cfg(),
        DeviceKind::Microphone => device.input_stream_cfg(),
    };
    config
        .map(|config| {
            config
                .buffers()
                .iter()
                .any(|buffer| buffer.number_channels > 0)
        })
        .unwrap_or(false)
}

fn native_device(
    device: Device,
    kind: DeviceKind,
    is_default: bool,
) -> Result<NativeDevice, String> {
    Ok(NativeDevice {
        id: device
            .uid()
            .map_err(|error| format!("{error:?}"))?
            .to_string(),
        name: device
            .name()
            .map_err(|error| format!("{error:?}"))?
            .to_string(),
        kind,
        is_default,
    })
}

pub struct MacDevices<P = NativeDevices> {
    provider: P,
}

impl MacDevices {
    pub fn new() -> Self {
        Self {
            provider: NativeDevices,
        }
    }
}

impl Default for MacDevices {
    fn default() -> Self {
        Self::new()
    }
}

impl<P: DeviceProvider> MacDevices<P> {
    pub fn with_provider(provider: P) -> Self {
        Self { provider }
    }

    pub fn list(&self) -> Vec<DeviceInfo> {
        let mut seen = HashSet::new();
        self.native_devices()
            .into_iter()
            .filter(|device| seen.insert((device.kind, device.id.clone())))
            .map(to_device_info)
            .collect()
    }

    pub fn default_system(&self) -> Option<DeviceInfo> {
        self.default(DeviceKind::SystemOutput)
    }

    pub fn default_microphone(&self) -> Option<DeviceInfo> {
        self.default(DeviceKind::Microphone)
    }

    pub fn resolve(&self, kind: DeviceKind, selection: &DeviceSelection) -> Option<DeviceInfo> {
        let devices = self.native_devices();
        let selected = match selection {
            DeviceSelection::Default => devices
                .into_iter()
                .find(|device| device.kind == kind && device.is_default),
            DeviceSelection::DeviceId(id) => devices
                .into_iter()
                .find(|device| device.kind == kind && device.id == *id),
        };
        selected.map(to_device_info)
    }

    fn default(&self, kind: DeviceKind) -> Option<DeviceInfo> {
        self.resolve(kind, &DeviceSelection::Default)
    }

    fn native_devices(&self) -> Vec<NativeDevice> {
        self.provider.list().unwrap_or_default()
    }
}

fn to_device_info(device: NativeDevice) -> DeviceInfo {
    DeviceInfo {
        id: device.id,
        name: device.name,
        is_default: device.is_default,
    }
}

pub(crate) struct DeviceWatcher {
    object: Obj,
    address: PropAddr,
    queue: cidre::arc::R<dispatch::Queue>,
    listener: cidre::arc::R<PropListenerBlock>,
}

impl DeviceWatcher {
    pub(crate) fn new(
        kind: SourceKind,
        selection: &DeviceSelection,
        events: Arc<dyn NativeStreamEvents>,
    ) -> Result<Self, String> {
        let (object, address) = match selection {
            DeviceSelection::Default => (
                *System::OBJ,
                match kind {
                    SourceKind::System => PropSelector::HW_DEFAULT_OUTPUT_DEVICE.global_addr(),
                    SourceKind::Microphone => PropSelector::HW_DEFAULT_INPUT_DEVICE.global_addr(),
                },
            ),
            DeviceSelection::DeviceId(id) => {
                let uid = cidre::cf::String::from_str(id);
                let device = Device::with_uid(&uid).map_err(|error| format!("{error:?}"))?;
                (device.0, PropSelector::DEVICE_IS_ALIVE.global_addr())
            }
        };
        let queue = dispatch::Queue::serial_with_ar_pool();
        let mut listener =
            blocks::EscBlock::new2(move |_count: u32, _addresses: *const PropAddr| {
                events.disconnected();
            });
        object
            .add_prop_listener_block(&address, Some(&queue), &mut listener)
            .map_err(|error| format!("{error:?}"))?;
        Ok(Self {
            object,
            address,
            queue,
            listener,
        })
    }
}

impl Drop for DeviceWatcher {
    fn drop(&mut self) {
        let _ = self.object.remove_prop_listener_block(
            &self.address,
            Some(&self.queue),
            &mut self.listener,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{DeviceKind, DeviceProvider, MacDevices, NativeDevice};
    use crate::DeviceInfo;

    #[derive(Clone)]
    struct FakeDevices(Vec<NativeDevice>);

    impl DeviceProvider for FakeDevices {
        fn list(&self) -> Result<Vec<NativeDevice>, String> {
            Ok(self.0.clone())
        }
    }

    fn devices() -> MacDevices<FakeDevices> {
        MacDevices::with_provider(FakeDevices(vec![
            NativeDevice {
                id: "speaker-default".into(),
                name: "MacBook Speakers".into(),
                kind: DeviceKind::SystemOutput,
                is_default: true,
            },
            NativeDevice {
                id: "mic-default".into(),
                name: "MacBook Microphone".into(),
                kind: DeviceKind::Microphone,
                is_default: true,
            },
            NativeDevice {
                id: "mic-usb".into(),
                name: "USB Microphone".into(),
                kind: DeviceKind::Microphone,
                is_default: false,
            },
        ]))
    }

    #[test]
    fn exposes_stable_ids_and_default_devices() {
        let devices = devices();

        assert_eq!(
            devices.list(),
            vec![
                DeviceInfo {
                    id: "speaker-default".into(),
                    name: "MacBook Speakers".into(),
                    is_default: true,
                },
                DeviceInfo {
                    id: "mic-default".into(),
                    name: "MacBook Microphone".into(),
                    is_default: true,
                },
                DeviceInfo {
                    id: "mic-usb".into(),
                    name: "USB Microphone".into(),
                    is_default: false,
                },
            ]
        );
        assert_eq!(devices.default_system().unwrap().id, "speaker-default");
        assert_eq!(devices.default_microphone().unwrap().id, "mic-default");
    }

    #[test]
    fn resolves_default_and_override_selections() {
        let devices = devices();

        assert_eq!(
            devices
                .resolve(DeviceKind::Microphone, &super::DeviceSelection::Default)
                .unwrap()
                .id,
            "mic-default"
        );
        assert_eq!(
            devices
                .resolve(
                    DeviceKind::Microphone,
                    &super::DeviceSelection::DeviceId("mic-usb".into())
                )
                .unwrap()
                .id,
            "mic-usb"
        );
        assert!(devices
            .resolve(
                DeviceKind::SystemOutput,
                &super::DeviceSelection::DeviceId("mic-usb".into())
            )
            .is_none());
    }
}
