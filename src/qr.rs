use std::sync::{Arc, RwLock};

use android_activity::AndroidApp;
use slint::Weak;

use crate::{
    MainWindow,
    camera::{CameraContext, CameraManager, ImageHandler, ImageRotation},
    java::{has_permission, phone_rotation, request_permission},
};

pub fn start_qr_scanner(
    main_window: Weak<MainWindow>,
    app: AndroidApp,
    camera_context: Arc<RwLock<Option<CameraContext>>>,
) -> bool {
    if camera_context.read().unwrap().is_some() {
        // Camera already setup and running.
        return true;
    }

    if !has_permission(&app) {
        request_permission(&app);

        // HACK: The `request_permission` is async. I'm unable to find a way to get a callback
        //       after the user has given the permission. So to prevent the code below to run
        //       before the user has given permissions, we indicate to the UI that it should
        //       go back to the start page. When the user then has granted the permission,
        //       the user can click to go to the "add-page" again (but this time the user
        //       already has the permission when ending up at this if-statement).
        return false;
    }

    let manager = CameraManager::new();

    let id = manager.get_camera_id();
    let stream_configuration = manager.get_stream_configuration(&id);

    // Rotation calculated with assumption that the camera is back-facing.
    let phone_rotation = phone_rotation(&app);
    let camera_rotation = manager.camera_rotation(&id);
    let image_rotation = ImageRotation::from_deg((camera_rotation - phone_rotation + 360) % 360);

    let mut image_reader = manager.create_image_reader(&stream_configuration);
    let image_handler = Box::into_raw(Box::new(ImageHandler::new(
        main_window,
        stream_configuration,
        image_rotation,
    )));
    image_reader.add_listener(image_handler);

    let window = image_reader.get_window();
    let target = window.create_target();
    let container = window.create_container();

    let mut device = manager.open_camera(&id);
    let mut session = device.create_session(container);
    let mut request = device.create_request();
    request.add_target(target);

    session.start(request);

    *camera_context.write().unwrap() = Some(CameraContext {
        session,
        device,
        manager,
        image_reader,
    });

    return true;
}

pub fn stop_qr_scanner(camera_context: Arc<RwLock<Option<CameraContext>>>) {
    *camera_context.write().unwrap() = None;
}
