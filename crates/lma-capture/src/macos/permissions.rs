use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

use cidre::{av, blocks, cg, ns};

use crate::PermissionState;

static SCREEN_REQUEST_DENIED: AtomicBool = AtomicBool::new(false);
const SCREEN_REQUEST_DENIED_KEY: &str = "capture.screen-recording.request-denied";

trait ScreenDenialHistory {
    fn denied(&self) -> bool;
    fn set_denied(&self, denied: bool);
}

#[derive(Clone, Copy)]
struct NativeScreenDenialHistory;

impl ScreenDenialHistory for NativeScreenDenialHistory {
    fn denied(&self) -> bool {
        let key = ns::String::with_str(SCREEN_REQUEST_DENIED_KEY);
        SCREEN_REQUEST_DENIED.load(Ordering::Acquire)
            || ns::UserDefaults::standard().get_u64(&key) == 1
    }

    fn set_denied(&self, denied: bool) {
        SCREEN_REQUEST_DENIED.store(denied, Ordering::Release);
        let key = ns::String::with_str(SCREEN_REQUEST_DENIED_KEY);
        ns::UserDefaults::standard().set_u64(&key, u64::from(denied));
    }
}

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
    fn request(&self, kind: PermissionKind) -> Result<NativeAuthorization, String>;
    fn open_settings(&self, kind: PermissionKind) -> Result<(), String>;
}

#[derive(Clone, Copy)]
pub struct NativePermissions;

impl PermissionProvider for NativePermissions {
    fn status(&self, kind: PermissionKind) -> NativeAuthorization {
        match kind {
            PermissionKind::ScreenRecording => screen_authorization_with_history(
                cg::screen_capture_access::preflight(),
                &NativeScreenDenialHistory,
            ),
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

    fn request(&self, kind: PermissionKind) -> Result<NativeAuthorization, String> {
        match kind {
            PermissionKind::ScreenRecording => {
                let granted = cg::screen_capture_access::request();
                remember_screen_request(granted, &NativeScreenDenialHistory);
                Ok(if granted {
                    NativeAuthorization::Authorized
                } else {
                    NativeAuthorization::Denied
                })
            }
            PermissionKind::Microphone => {
                let (sender, receiver) = mpsc::sync_channel(1);
                let mut completion = blocks::SendBlock::new1(move |granted: bool| {
                    let _ = sender.send(granted);
                });
                av::CaptureDevice::request_access_for_media_type_ch(
                    av::MediaType::audio(),
                    &mut completion,
                )
                .map_err(|error| format!("{error:?}"))?;
                let granted = receiver
                    .recv_timeout(Duration::from_secs(120))
                    .map_err(|_| "microphone permission request timed out".to_owned())?;
                Ok(if granted {
                    NativeAuthorization::Authorized
                } else {
                    NativeAuthorization::Denied
                })
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

fn screen_authorization(
    preflight_granted: bool,
    previous_request_was_denied: bool,
) -> NativeAuthorization {
    if preflight_granted {
        NativeAuthorization::Authorized
    } else if previous_request_was_denied {
        NativeAuthorization::Denied
    } else {
        NativeAuthorization::NotDetermined
    }
}

fn screen_authorization_with_history(
    preflight_granted: bool,
    history: &impl ScreenDenialHistory,
) -> NativeAuthorization {
    if preflight_granted {
        history.set_denied(false);
    }
    screen_authorization(preflight_granted, history.denied())
}

fn remember_screen_request(granted: bool, history: &impl ScreenDenialHistory) {
    history.set_denied(!granted);
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

    pub fn request_access(&self) -> Result<PermissionState, String> {
        let authorization = match self.provider.status(self.kind) {
            NativeAuthorization::NotDetermined => self.provider.request(self.kind)?,
            authorization => authorization,
        };
        Ok(match authorization {
            NativeAuthorization::Authorized => PermissionState::Granted,
            NativeAuthorization::Denied => PermissionState::Denied,
            NativeAuthorization::NotDetermined => PermissionState::Unknown,
        })
    }

    pub fn ensure_access(&self) -> Result<(), String> {
        if self.request_access()? == PermissionState::Granted {
            return Ok(());
        }
        let name = match self.kind {
            PermissionKind::ScreenRecording => "Screen Recording",
            PermissionKind::Microphone => "Microphone",
        };
        self.provider
            .open_settings(self.kind)
            .map_err(|error| format!("{name} permission was not granted: {error}"))?;
        Err(format!(
            "{name} permission was not granted; System Settings was opened"
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    use crate::PermissionState;

    use super::{MacPermissions, NativeAuthorization, PermissionKind, PermissionProvider};

    #[derive(Clone)]
    struct FakePermissions {
        screen: NativeAuthorization,
        microphone: NativeAuthorization,
        requested: Rc<RefCell<Vec<PermissionKind>>>,
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

        fn request(&self, kind: PermissionKind) -> Result<NativeAuthorization, String> {
            self.requested.borrow_mut().push(kind);
            Ok(NativeAuthorization::Denied)
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
                    requested: Rc::new(RefCell::new(Vec::new())),
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
            requested: Rc::new(RefCell::new(Vec::new())),
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

    #[test]
    fn failed_screen_preflight_is_conservatively_unknown() {
        assert_eq!(
            super::screen_authorization(false, false),
            NativeAuthorization::NotDetermined
        );
    }

    #[test]
    fn failed_screen_request_is_remembered_as_denied() {
        assert_eq!(
            super::screen_authorization(false, true),
            NativeAuthorization::Denied
        );
    }

    #[test]
    fn screen_denial_history_survives_a_new_permission_adapter() {
        #[derive(Clone)]
        struct SharedHistory(Rc<Cell<bool>>);

        impl super::ScreenDenialHistory for SharedHistory {
            fn denied(&self) -> bool {
                self.0.get()
            }

            fn set_denied(&self, denied: bool) {
                self.0.set(denied);
            }
        }

        let storage = Rc::new(Cell::new(false));
        let first_launch = SharedHistory(storage.clone());
        super::remember_screen_request(false, &first_launch);
        let next_launch = SharedHistory(storage);

        assert_eq!(
            super::screen_authorization_with_history(false, &next_launch),
            NativeAuthorization::Denied
        );
        super::remember_screen_request(true, &next_launch);
        assert_eq!(
            super::screen_authorization_with_history(true, &next_launch),
            NativeAuthorization::Authorized
        );
        assert!(!super::ScreenDenialHistory::denied(&next_launch));
    }

    #[test]
    fn denied_permission_request_opens_settings_and_returns_an_error() {
        let opened = Rc::new(RefCell::new(Vec::new()));
        let requested = Rc::new(RefCell::new(Vec::new()));
        let permissions = MacPermissions::with_provider(
            PermissionKind::ScreenRecording,
            FakePermissions {
                screen: NativeAuthorization::NotDetermined,
                microphone: NativeAuthorization::Authorized,
                requested: requested.clone(),
                opened: opened.clone(),
            },
        );

        let error = permissions.ensure_access().unwrap_err();

        assert!(error.contains("Screen Recording permission was not granted"));
        assert_eq!(*requested.borrow(), [PermissionKind::ScreenRecording]);
        assert_eq!(*opened.borrow(), [PermissionKind::ScreenRecording]);
    }

    #[test]
    fn explicit_permission_request_reports_denial_without_opening_settings() {
        let opened = Rc::new(RefCell::new(Vec::new()));
        let requested = Rc::new(RefCell::new(Vec::new()));
        let permissions = MacPermissions::with_provider(
            PermissionKind::ScreenRecording,
            FakePermissions {
                screen: NativeAuthorization::NotDetermined,
                microphone: NativeAuthorization::Authorized,
                requested: requested.clone(),
                opened: opened.clone(),
            },
        );

        assert_eq!(
            permissions.request_access().unwrap(),
            PermissionState::Denied
        );
        assert_eq!(*requested.borrow(), [PermissionKind::ScreenRecording]);
        assert!(opened.borrow().is_empty());
    }
}
