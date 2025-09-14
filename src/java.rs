use android_activity::AndroidApp;
use jni::{
    AttachGuard, JavaVM,
    objects::{JObject, JValue, JValueGen},
};

pub static PERMISSION_CAMERA: &'static str = "android.permission.CAMERA";
pub static WINDOW_SERVICE: &'static str = "window";

// https://github.com/slint-ui/slint/discussions/5692#discussioncomment-11601025
// https://github.com/bit-shift-io/bike-aid/blob/01a864a6c9119487bded074c425558082702a908/old/app-rs/src/android.rs#L166
// https://github.com/slint-ui/slint/blob/9a882dd17fcf75968d7116e2115774825e02bb3a/internal/backends/android-activity/javahelper.rs#L17
pub fn has_permission(app: &AndroidApp) -> bool {
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr() as *mut _).unwrap() };
    let mut env = vm.attach_current_thread().unwrap();
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr() as *mut _) };

    let permission_arg = env.new_string(PERMISSION_CAMERA).unwrap();

    env.call_method(
        activity,
        "checkSelfPermission",
        "(Ljava/lang/String;)I",
        &[(&permission_arg).into()],
    )
    .unwrap()
    .i()
    .unwrap()
        == 0
}

pub fn request_permission(app: &AndroidApp) {
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr() as *mut _).unwrap() };
    let mut env = vm.attach_current_thread().unwrap();
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr() as *mut _) };

    let string_class = env.find_class("java/lang/String").unwrap();
    let permission_arg = env.new_string(PERMISSION_CAMERA).unwrap();
    let permissions_arg = env
        .new_object_array(1, string_class, permission_arg)
        .unwrap();

    env.call_method(
        activity,
        "requestPermissions",
        "([Ljava/lang/String;I)V",
        &[(&permissions_arg).into(), JValue::from(0)],
    )
    .unwrap();
}

pub fn phone_rotation(app: &AndroidApp) -> i32 {
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr() as *mut _).unwrap() };
    let mut env = vm.attach_current_thread().unwrap();
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr() as *mut _) };

    let service_arg = env.new_string(WINDOW_SERVICE).unwrap();
    let window_service = env
        .call_method(
            activity,
            "getSystemService",
            "(Ljava/lang/String;)Ljava/lang/Object;",
            &[(&service_arg).into()],
        )
        .unwrap()
        .l()
        .unwrap();

    let display = env
        .call_method(
            window_service,
            "getDefaultDisplay",
            "()Landroid/view/Display;",
            &[],
        )
        .unwrap()
        .l()
        .unwrap();

    env.call_method(display, "getRotation", "()I", &[])
        .unwrap()
        .i()
        .unwrap()
}
