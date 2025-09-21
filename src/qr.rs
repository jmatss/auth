use std::rc::Rc;

use jni::{
    JNIEnv, JavaVM,
    objects::{JByteArray, JClass},
    sys::{jint, jlong},
};
use rqrr::PreparedImage;
use slint::{Rgb8Pixel, SharedPixelBuffer};
use totp_rs::TOTP;
use yuv::{RotationMode, YuvPlanarImage, YuvRange, YuvStandardMatrix, rotate_rgb, yuv420_to_rgb};

use crate::{
    AppState, Page,
    codes::CodeMessage,
    java::{has_permission, request_permission},
};

pub fn start_qr_scanner(state: Rc<AppState>, state_raw: *mut Rc<AppState>) -> bool {
    let vm = unsafe { JavaVM::from_raw(state.app.vm_as_ptr() as *mut _).unwrap() };
    let mut env = vm.attach_current_thread().unwrap();

    if state.java_helpers.is_camera_running(&mut env) {
        return true;
    }

    if !has_permission(&state.app) {
        request_permission(&state.app);

        // HACK: The `request_permission` is async. I'm unable to find a way to get a callback
        //       after the user has given the permission. So to prevent the code below to run
        //       before the user has given permissions, we indicate to the UI that it should
        //       go back to the start page. When the user then has granted the permission,
        //       the user can click to go to the "add-page" again (but this time the user
        //       already has the permission when ending up at this if-statement).
        return false;
    }

    state.java_helpers.start_camera(&mut env, state_raw);

    return true;
}

pub fn stop_qr_scanner(state: Rc<AppState>) {
    let vm = unsafe { JavaVM::from_raw(state.app.vm_as_ptr() as *mut _).unwrap() };
    let mut env = vm.attach_current_thread().unwrap();

    if state.java_helpers.is_camera_running(&mut env) {
        let _ = state.java_helpers.stop_camera(&mut env);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_CameraHelper_handleImage<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    app_state: jlong,
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
    let state = unsafe { (app_state as *mut Rc<AppState>).as_ref().unwrap() };

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

    let pixel_buffer = android_yuv_to_slint_rgb(yuv_image, rotation, width, height);
    let otp_auth_url = parse_qr_code(&y_plane, width as usize, height as usize);
    let url_with_sender = otp_auth_url.map(|a| (a, state.sender.clone()));

    // https://github.com/slint-ui/slint/issues/1649
    state
        .main_window
        .upgrade_in_event_loop(move |window| {
            window.set_camera(slint::Image::from_rgb8(pixel_buffer));

            if let Some((url, sender)) = url_with_sender {
                sender.send(CodeMessage::Add(url)).unwrap();
                window.invoke_navigate_to_page(Page::Start);
            }
        })
        .unwrap();
}

fn android_yuv_to_slint_rgb(
    yuv_image: YuvPlanarImage<'_, u8>,
    rotation: i32,
    width: i32,
    height: i32,
) -> SharedPixelBuffer<Rgb8Pixel> {
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

    SharedPixelBuffer::<Rgb8Pixel>::clone_from_slice(
        &rgb_bytes_rotated,
        dst_width as u32,
        dst_height as u32,
    )
}

pub fn parse_qr_code(y_plane: &[u8], width: usize, height: usize) -> Option<String> {
    // We ignore that `y_plane` isn't rotated correctly, the `rqrr` reader is able to read it in any 90deg orientation.
    let mut qr_image =
        PreparedImage::prepare_from_greyscale(width, height, |x, y| y_plane[x + y * width]);

    let qr_grids = qr_image.detect_grids();
    for qr_grid in qr_grids {
        match qr_grid.decode() {
            Ok((_, qr_value)) => return Some(qr_value),
            Err(err) => eprintln!("Unable to read possible QR code: {}", err),
        }
    }

    None
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
