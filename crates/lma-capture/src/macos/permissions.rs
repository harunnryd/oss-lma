use cidre::{av, cg, ns};

use crate::PermissionState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionKind {
    ScreenRecording,
    Microphone,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeAuthorization {
    Authorized,
    Denied,
    NotDetermined,
}

pub trait PermissionProvider {
    fn status(&self, kind: PermissionKind) -> NativeAuthorization;
    fn open_settings(&self, kind: PermissionKind) -> Result<(), String>;
}

#[derive(Clone, Copy)]
pub struct NativePermissions;

impl PermissionProvider for NativePermissions {
    fn status(&self, kind: PermissionKind) -> NativeAuthorization {
        match kind {
            PermissionKind::ScreenRecording => {
                if cg::screen_capture_access::preflight() {
                    NativeAuthorization::Authorized
                } else {
                    NativeAuthorization::Denied
                }
            }
            PermissionKind::Microphone => {
                let status =
                    av::CaptureDevice::authorization_status_for_media_type(av::MediaType::audio());
                match status {
                    Ok(av::AuthorizationStatus::Authorized) => NativeAuthorization::Authorized,
                    Ok(av::AuthorizationStatus::NotDetermined) => {
                        NativeAuthorization::NotDetermined
                    }
                    Ok(av::AuthorizationStatus::Restricted | av::AuthorizationStatus::Denied)
                    | Err(_) => NativeAuthorization::Denied,
                }
            }
        }
    }

    fn open_settings(&self, kind: PermissionKind) -> Result<(), String> {
        let pane = match kind {
            PermissionKind::ScreenRecording => "Privacy_ScreenCapture",
            PermissionKind::Microphone => "Privacy_Microphone",
        };
        let url = ns::Url::with_str(&format!(
            "x-apple.systempreferences:com.apple.preference.security?{pane}"
        ))
        .ok_or_else(|| "invalid System Settings URL".to_owned())?;
        if ns::Workspace::shared().open_url(&url) {
            Ok(())
        } else {
            Err("System Settings could not be opened".to_owned())
        }
    }
}

pub struct MacPermissions<P = NativePermissions> {
    kind: PermissionKind,
    provider: P,
}

impl MacPermissions {
    pub fn screen_recording() -> Self {
        Self {
            kind: PermissionKind::ScreenRecording,
            provider: NativePermissions,
        }
    }

    pub fn microphone() -> Self {
        Self {
            kind: PermissionKind::Microphone,
            provider: NativePermissions,
        }
    }
}

impl<P: PermissionProvider> MacPermissions<P> {
    pub fn with_provider(kind: PermissionKind, provider: P) -> Self {
        Self { kind, provider }
    }

    pub fn status(&self) -> PermissionState {
        match self.provider.status(self.kind) {
            NativeAuthorization::Authorized => PermissionState::Granted,
            NativeAuthorization::Denied => PermissionState::Denied,
            NativeAuthorization::NotDetermined => PermissionState::Unknown,
        }
    }

    pub fn open_settings(&self) -> Result<(), String> {
        self.provider.open_settings(self.kind)
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use crate::PermissionState;

    use super::{MacPermissions, NativeAuthorization, PermissionKind, PermissionProvider};

    #[derive(Clone)]
    struct FakePermissions {
        screen: NativeAuthorization,
        microphone: NativeAuthorization,
        opened: Rc<RefCell<Vec<PermissionKind>>>,
    }

    impl PermissionProvider for FakePermissions {
        fn status(&self, kind: PermissionKind) -> NativeAuthorization {
            match kind {
                PermissionKind::ScreenRecording => self.screen,
                PermissionKind::Microphone => self.microphone,
            }
        }

        fn open_settings(&self, kind: PermissionKind) -> Result<(), String> {
            self.opened.borrow_mut().push(kind);
            Ok(())
        }
    }

    #[test]
    fn maps_native_authorization_states() {
        let cases = [
            (NativeAuthorization::Authorized, PermissionState::Granted),
            (NativeAuthorization::Denied, PermissionState::Denied),
            (NativeAuthorization::NotDetermined, PermissionState::Unknown),
        ];

        for (native, expected) in cases {
            let permissions = MacPermissions::with_provider(
                PermissionKind::Microphone,
                FakePermissions {
                    screen: NativeAuthorization::Authorized,
                    microphone: native,
                    opened: Rc::new(RefCell::new(Vec::new())),
                },
            );
            assert_eq!(permissions.status(), expected);
        }
    }

    #[test]
    fn opens_the_settings_pane_for_the_selected_permission() {
        let opened = Rc::new(RefCell::new(Vec::new()));
        let provider = FakePermissions {
            screen: NativeAuthorization::Denied,
            microphone: NativeAuthorization::Denied,
            opened: opened.clone(),
        };
        let screen =
            MacPermissions::with_provider(PermissionKind::ScreenRecording, provider.clone());
        let microphone = MacPermissions::with_provider(PermissionKind::Microphone, provider);

        screen.open_settings().unwrap();
        microphone.open_settings().unwrap();

        assert_eq!(
            *opened.borrow(),
            [PermissionKind::ScreenRecording, PermissionKind::Microphone]
        );
    }
}
