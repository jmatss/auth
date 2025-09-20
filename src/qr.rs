use std::sync::{Arc, RwLock};

use android_activity::AndroidApp;
use jni::{
    JNIEnv, JavaVM, NativeMethod,
    objects::{GlobalRef, JByteArray, JClass, JObject, JValue},
    sys::{jint, jlong},
};
use slint::{Rgb8Pixel, SharedPixelBuffer, Weak};
use yuv::{RotationMode, YuvPlanarImage, YuvRange, YuvStandardMatrix, rotate_rgb, yuv420_to_rgb};

use crate::{
    MainWindow,
    java::{has_permission, request_permission},
};

pub fn start_qr_scanner(
    app: AndroidApp,
    java_camera_helper: Arc<RwLock<Option<GlobalRef>>>,
    main_window: Weak<MainWindow>,
) -> bool {
    if java_camera_helper.read().unwrap().is_some() {
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

    let main_window_raw = Box::into_raw(Box::new(main_window));
    *java_camera_helper.write().unwrap() = Some(create_camera_helper(&app, main_window_raw));

    return true;
}

pub fn stop_qr_scanner(app: AndroidApp, java_camera_helper: Arc<RwLock<Option<GlobalRef>>>) {
    if let Some(camera_helper) = &*java_camera_helper.read().unwrap() {
        let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr() as *mut _).unwrap() };
        let mut env = vm.attach_current_thread().unwrap();

        let main_window = env
            .call_method(camera_helper, "stop", "()J", &[])
            .unwrap()
            .j()
            .unwrap();

        unsafe { std::mem::drop(Box::from_raw(main_window as *mut Weak<MainWindow>)) }
    }

    *java_camera_helper.write().unwrap() = None;
}

fn create_camera_helper(app: &AndroidApp, slint_main_window: *mut Weak<MainWindow>) -> GlobalRef {
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr() as *mut _).unwrap() };
    let mut env = vm.attach_current_thread().unwrap();
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr() as *mut _) };

    let dex_data = include_bytes!(concat!(env!("OUT_DIR"), "/classes.dex"));

    let dex_buffer = unsafe {
        env.new_direct_byte_buffer(dex_data.as_ptr() as *mut _, dex_data.len())
            .unwrap()
    };

    let class_loader = env
        .call_method(
            &activity,
            "getClassLoader",
            "()Ljava/lang/ClassLoader;",
            &[],
        )
        .unwrap()
        .l()
        .unwrap();

    let dex_class_loader = env
        .new_object(
            "dalvik/system/InMemoryDexClassLoader",
            "(Ljava/nio/ByteBuffer;Ljava/lang/ClassLoader;)V",
            &[(&dex_buffer).into(), (&class_loader).into()],
        )
        .unwrap();

    let class_name = env.new_string("CameraHelper").unwrap();
    let camera_helper_class: JClass = env
        .call_method(
            dex_class_loader,
            "findClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[(&class_name).into()],
        )
        .unwrap()
        .l()
        .unwrap()
        .into();

    env.register_native_methods(
        &camera_helper_class,
        &[NativeMethod {
            name: "handleImage".into(),
            sig: "(J[BI[BI[BIIII)V".into(),
            fn_ptr: Java_CameraHelper_handleImage as *mut _,
        }],
    )
    .unwrap();

    let camera_helper = env
        .new_object(
            camera_helper_class,
            "(Landroid/app/Activity;J)V",
            &[(&activity).into(), JValue::Long(slint_main_window as jlong)],
        )
        .unwrap();

    env.call_method(&camera_helper, "start", "()V", &[])
        .unwrap();

    env.new_global_ref(&camera_helper).unwrap()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_CameraHelper_handleImage<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    main_window: jlong,
    y_plane: JByteArray<'local>,
    y_stride: jint,
    u_plane: JByteArray<'local>,
    u_stride: jint,
    v_plane: JByteArray<'local>,
    v_stride: jint,
    rotation: jint,
    width: jint,
    height: jint,
) {
    let slint_main_window = unsafe { (main_window as *const Weak<MainWindow>).as_ref().unwrap() };

    let y_plane = env.convert_byte_array(y_plane).unwrap();
    let u_plane = env.convert_byte_array(u_plane).unwrap();
    let v_plane = env.convert_byte_array(v_plane).unwrap();

    let yuv_image = YuvPlanarImage {
        y_plane: &y_plane,
        y_stride: y_stride as u32,
        u_plane: &u_plane,
        u_stride: u_stride as u32,
        v_plane: &v_plane,
        v_stride: v_stride as u32,
        width: width as u32,
        height: height as u32,
    };

    let channels = 3;
    let mut rgb_bytes = vec![0; (width * height * channels) as usize];

    yuv420_to_rgb(
        &yuv_image,
        &mut rgb_bytes,
        (width * channels) as u32,
        YuvRange::Full,
        YuvStandardMatrix::Bt601,
    )
    .unwrap();

    let (rotation_mode, dst_width, dst_height) = rotation_mode(rotation, width, height);

    let rgb_bytes_rotated = if let Some(rotation_mode) = rotation_mode {
        let mut rgb_bytes_rotated = vec![0; (width * height * channels) as usize];
        rotate_rgb(
            &rgb_bytes,
            (width * channels) as usize,
            &mut rgb_bytes_rotated,
            (dst_width * channels) as usize,
            width as usize,
            height as usize,
            rotation_mode,
        )
        .unwrap();
        rgb_bytes_rotated
    } else {
        rgb_bytes
    };

    let pixel_buffer = SharedPixelBuffer::<Rgb8Pixel>::clone_from_slice(
        &rgb_bytes_rotated,
        dst_width as u32,
        dst_height as u32,
    );

    // https://github.com/slint-ui/slint/issues/1649
    slint_main_window
        .upgrade_in_event_loop(move |window| {
            window.set_camera(slint::Image::from_rgb8(pixel_buffer));
        })
        .unwrap();
}

pub fn rotation_mode(rotation: i32, width: i32, height: i32) -> (Option<RotationMode>, i32, i32) {
    // The `yuv` crate rotates the images counter-clockwise? So need to swap 90 & 270.
    match rotation {
        45..135 => (Some(RotationMode::Rotate270), height, width),
        135..225 => (Some(RotationMode::Rotate180), width, height),
        225..315 => (Some(RotationMode::Rotate90), height, width),
        // 0..45 && 315..360
        _ => (None, width, height),
    }
}
