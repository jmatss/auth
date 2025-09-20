use std::rc::Rc;

use android_activity::AndroidApp;
use jni::{
    AttachGuard, JavaVM, NativeMethod,
    objects::{GlobalRef, JClass, JObject, JObjectArray, JValue},
    sys::jlong,
};

use crate::{AppState, qr::Java_CameraHelper_handleImage};

pub static PERMISSION_CAMERA: &'static str = "android.permission.CAMERA";

// https://github.com/slint-ui/slint/discussions/5692#discussioncomment-11601025
// https://github.com/bit-shift-io/bike-aid/blob/01a864a6c9119487bded074c425558082702a908/old/app-rs/src/android.rs#L166
// https://github.com/slint-ui/slint/blob/9a882dd17fcf75968d7116e2115774825e02bb3a/internal/backends/android-activity/javahelper.rs#L17

pub struct JavaHelpers {
    camera: GlobalRef,
    otp_auth: GlobalRef,
}

impl JavaHelpers {
    pub fn is_camera_running(&self, env: &mut AttachGuard) -> bool {
        env.call_method(&self.camera, "isRunning", "()Z", &[])
            .unwrap()
            .z()
            .unwrap()
    }

    pub fn start_camera(&self, env: &mut AttachGuard, state: *mut Rc<AppState>) {
        env.call_method(
            &self.camera,
            "start",
            "(J)V",
            &[JValue::Long(state as jlong)],
        )
        .unwrap();
    }

    pub fn stop_camera(&self, env: &mut AttachGuard) {
        env.call_method(&self.camera, "stop", "()V", &[]).unwrap();
    }

    pub fn write_url_to_disk(&self, env: &mut AttachGuard, url: &str) {
        let url_arg = env.new_string(url).unwrap();
        env.call_method(
            &self.otp_auth,
            "add",
            "(Ljava/lang/String;)V",
            &[(&url_arg).into()],
        )
        .unwrap();
    }

    pub fn remove_url_from_disk(&self, env: &mut AttachGuard, url: &str) {
        let url_arg = env.new_string(url).unwrap();
        env.call_method(
            &self.otp_auth,
            "remove",
            "(Ljava/lang/String;)V",
            &[(&url_arg).into()],
        )
        .unwrap();
    }

    pub fn edit_url_on_disk(&self, env: &mut AttachGuard, old_url: &str, new_url: &str) {
        let old_url_arg = env.new_string(old_url).unwrap();
        let new_url_arg = env.new_string(new_url).unwrap();
        env.call_method(
            &self.otp_auth,
            "edit",
            "(Ljava/lang/String;Ljava/lang/String;)V",
            &[(&old_url_arg).into(), (&new_url_arg).into()],
        )
        .unwrap();
    }

    pub fn get_urls_from_disk(&self, env: &mut AttachGuard) -> Vec<String> {
        let urls_array: JObjectArray = env
            .call_method(&self.otp_auth, "get", "()[Ljava/lang/String;", &[])
            .unwrap()
            .l()
            .unwrap()
            .into();

        let length = env.get_array_length(&urls_array).unwrap();
        let mut urls = Vec::with_capacity(length as usize);

        for i in 0..length {
            let url_object = env.get_object_array_element(&urls_array, i).unwrap();
            let url: String = env.get_string((&url_object).into()).unwrap().into();
            urls.push(url);
        }

        urls
    }
}

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

pub fn load_helper_objects(app: &AndroidApp) -> JavaHelpers {
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr() as *mut _).unwrap() };
    let mut env = vm.attach_current_thread().unwrap();
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr() as *mut _) };

    let dex_class_loader = load_dex_class_loader(&mut env, &activity);
    let camera = load_camera_helper(&mut env, &dex_class_loader, &activity);
    let otp_auth = load_otp_auth_helper(&mut env, &dex_class_loader, &activity);

    JavaHelpers { camera, otp_auth }
}

fn load_dex_class_loader<'local>(
    env: &mut AttachGuard<'local>,
    activity: &JObject,
) -> JObject<'local> {
    let dex_data = include_bytes!(concat!(env!("OUT_DIR"), "/classes.dex"));

    let dex_buffer = unsafe {
        env.new_direct_byte_buffer(dex_data.as_ptr() as *mut _, dex_data.len())
            .unwrap()
    };

    let class_loader = env
        .call_method(activity, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
        .unwrap()
        .l()
        .unwrap();

    env.new_object(
        "dalvik/system/InMemoryDexClassLoader",
        "(Ljava/nio/ByteBuffer;Ljava/lang/ClassLoader;)V",
        &[(&dex_buffer).into(), (&class_loader).into()],
    )
    .unwrap()
}

pub fn load_camera_helper<'local>(
    env: &mut AttachGuard<'local>,
    dex_class_loader: &JObject,
    activity: &JObject,
) -> GlobalRef {
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
            "(Landroid/app/Activity;)V",
            &[(&activity).into()],
        )
        .unwrap();

    env.new_global_ref(&camera_helper).unwrap()
}

fn load_otp_auth_helper(
    env: &mut AttachGuard,
    dex_class_loader: &JObject,
    activity: &JObject,
) -> GlobalRef {
    let class_name = env.new_string("OtpAuthHelper").unwrap();
    let otp_auth_helper_class: JClass = env
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

    let otp_auth_helper = env
        .new_object(
            otp_auth_helper_class,
            "(Landroid/app/Activity;)V",
            &[(&activity).into()],
        )
        .unwrap();

    env.new_global_ref(&otp_auth_helper).unwrap()
}
