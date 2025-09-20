use std::{cell::RefCell, rc::Rc};

use android_activity::MainEvent;
use async_compat::Compat;
use i_slint_backend_android_activity::AndroidPlatform;
use i_slint_core::{items::FocusReason, window::WindowInner};

use slint::{ModelRc, VecModel, Weak, android::AndroidApp};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

use crate::{
    codes::{CodeMessage, code_handler},
    java::{JavaHelpers, load_helper_objects},
    qr::{start_qr_scanner, stop_qr_scanner},
};

mod codes;
mod java;
mod qr;

slint::include_modules!();

pub struct AppState {
    app: AndroidApp,
    main_window: Weak<MainWindow>,
    codes: Rc<VecModel<Code>>,
    java_helpers: JavaHelpers,
    /// The `codes_handler` has the corresponding Receiver.
    sender: UnboundedSender<CodeMessage>,
}

#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    // HACK: Slint only reacts to key events when a component has focus. If no component has focus,
    //       the event is ignored and can't be handled in the Slint user code.
    //
    //       This is problematic for Androids `Key.Back`. So for example if a user modifies a
    //       textbox and then removes the focus from the textbox (e.g. by clicking `Key.Back`), the
    //       application will not be able to handle the upcoming key events. So if the user presses
    //       `Key.Back`, the android app will always be closed. This is not always the behaviour we
    //       want since sometimes the `Key.Back` should navigate back to the previous page of the
    //       application.
    //
    //       This hack makes sure that there is always a component that has focus before a input
    //       event is handled. So if no component has focus, this code sets the focus on the root
    //       component window. We can then handle all key events in the Slint user code (from the
    //       root component window).
    //
    //       The "platform" needs to be created/setup before the `MainWindow` is created, so need
    //       to wrap RefCell.
    let main_window_rc: Rc<RefCell<Option<MainWindow>>> = Rc::new(RefCell::new(None));
    let main_window_clone = main_window_rc.clone();

    slint::platform::set_platform(Box::new(AndroidPlatform::new_with_event_listener(
        app.clone(),
        move |e| match e {
            i_slint_backend_android_activity::android_activity::PollEvent::Main(
                MainEvent::InputAvailable,
            ) => {
                if let Some(window) = main_window_clone.as_ref().borrow().as_ref() {
                    let inner_window = WindowInner::from_pub(window.window());
                    if inner_window.focus_item.borrow().upgrade().is_none() {
                        let root_window_component = &inner_window.window_item_rc().unwrap();
                        inner_window.set_focus_item(
                            root_window_component,
                            true,
                            FocusReason::WindowActivation,
                        );
                    }
                }
            }
            _ => {}
        },
    )))
    .unwrap();

    let main_window = MainWindow::new().unwrap();

    let java_helpers = load_helper_objects(&app);
    let codes: Rc<VecModel<_>> = Rc::new(VecModel::from(Vec::new()));
    let (sender, receiver) = unbounded_channel::<CodeMessage>();

    let state = Rc::new(AppState {
        app,
        codes: Rc::clone(&codes),
        main_window: main_window.as_weak(),
        java_helpers,
        sender,
    });
    // Make raw pointer that can be sent over to Java code. We need to make sure to
    // drop this leaked Box at the end of this function.
    let state_raw = Box::into_raw(Box::new(Rc::clone(&state)));

    let state_clone = Rc::clone(&state);
    let _ =
        slint::spawn_local(Compat::new(code_handler(Rc::clone(&state_clone), receiver))).unwrap();

    main_window.set_codes(ModelRc::from(Rc::clone(&codes)));
    let state_clone = Rc::clone(&state);
    main_window.on_start_qr_scanner(move || start_qr_scanner(Rc::clone(&state_clone), state_raw));
    let state_clone = Rc::clone(&state);
    main_window.on_stop_qr_scanner(move || stop_qr_scanner(Rc::clone(&state_clone)));

    *main_window_rc.borrow_mut() = Some(main_window);
    main_window_rc.borrow().as_ref().unwrap().run().unwrap();

    std::mem::drop(unsafe { Box::from_raw(state_raw) });
}
