use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, RwLock},
};

use android_activity::MainEvent;
use i_slint_backend_android_activity::AndroidPlatform;
use i_slint_core::{items::FocusReason, window::WindowInner};
use slint::android::AndroidApp;

use crate::{
    camera::CameraContext,
    codes::{add_code, load_codes},
    qr::{start_qr_scanner, stop_qr_scanner},
};

mod camera;
mod codes;
mod java;
mod qr;

slint::include_modules!();

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
    let main_window_add = main_window.as_weak();
    let main_window_qr = main_window.as_weak();

    let camera_context: Arc<RwLock<Option<CameraContext>>> = Arc::default();
    let camera_context_start = Arc::clone(&camera_context);
    let camera_context_stop = Arc::clone(&camera_context);

    main_window.set_codes(load_codes());
    main_window.on_add_code(move |name, secret| add_code(&main_window_add, name, secret));
    main_window.on_start_qr_scanner(move || {
        start_qr_scanner(
            main_window_qr.clone(),
            app.clone(),
            Arc::clone(&camera_context_start),
        )
    });
    main_window.on_stop_qr_scanner(move || stop_qr_scanner(Arc::clone(&camera_context_stop)));

    *main_window_rc.borrow_mut() = Some(main_window);
    main_window_rc.borrow().as_ref().unwrap().run().unwrap();
}
