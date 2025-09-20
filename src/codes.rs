use std::{
    rc::Rc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use android_activity::AndroidApp;
use jni::JavaVM;
use slint::Model;
use tokio::{sync::mpsc::UnboundedReceiver, time::timeout};
use totp_rs::TOTP;

use crate::{AppState, Code, java::JavaHelpers};

pub enum CodeMessage {
    /// The `String` is the URL of the added code.
    Add(String),
    /// The `String` is the URL of the removed code.
    Remove(String),
    /// The `String` is the old URL. The second `String` is the new URL.
    Edit(String, String),
}

pub async fn code_handler(state: Rc<AppState>, mut reciver: UnboundedReceiver<CodeMessage>) {
    let mut unique_idx = 0;

    let mut totps = load_totps(state.app.clone(), &state.java_helpers);

    for totp in totps.iter() {
        state.codes.push(totp_to_code(unique_idx, totp));
        unique_idx += 1;
    }

    loop {
        let unix_time_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        for (row, (code, totp)) in state.codes.iter().zip(&totps).enumerate() {
            if unix_time_ms > code.valid_until_unix_time {
                let new_code = totp_to_code(code.unique_idx, totp);
                state.codes.set_row_data(row, new_code);
            }
        }

        match timeout(Duration::from_millis(500), reciver.recv()).await {
            Ok(Some(CodeMessage::Add(url))) => {
                let vm = unsafe { JavaVM::from_raw(state.app.vm_as_ptr() as *mut _).unwrap() };
                let mut env = vm.attach_current_thread().unwrap();

                state.java_helpers.write_url_to_disk(&mut env, &url);

                let totp = TOTP::from_url(&url).unwrap();
                let code = totp_to_code(unique_idx, &totp);
                totps.push(totp);
                state.codes.push(code);

                unique_idx += 1;
            }
            Ok(Some(CodeMessage::Remove(url))) => {
                let vm = unsafe { JavaVM::from_raw(state.app.vm_as_ptr() as *mut _).unwrap() };
                let mut env = vm.attach_current_thread().unwrap();

                state.java_helpers.remove_url_from_disk(&mut env, &url);
            }
            Ok(Some(CodeMessage::Edit(old_url, new_url))) => {
                let vm = unsafe { JavaVM::from_raw(state.app.vm_as_ptr() as *mut _).unwrap() };
                let mut env = vm.attach_current_thread().unwrap();

                state
                    .java_helpers
                    .edit_url_on_disk(&mut env, &old_url, &new_url);
            }
            // Timeout. This is expected, continue loop as normal.
            Err(_) => (),
            // The channel is closed. Break out of inifinite loop.
            Ok(None) => break,
        }
    }
}

fn load_totps(app: AndroidApp, java_helpers: &JavaHelpers) -> Vec<TOTP> {
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr() as *mut _).unwrap() };
    let mut env = vm.attach_current_thread().unwrap();

    java_helpers
        .get_urls_from_disk(&mut env)
        .iter()
        .map(|url| TOTP::from_url(url).unwrap())
        .collect::<Vec<_>>()
}

fn totp_to_code(unique_idx: i32, totp: &TOTP) -> Code {
    let unix_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();

    let valid_duration_secs = totp.step - unix_time.as_secs() % totp.step;
    let valid_until_unix_time =
        (unix_time + Duration::from_secs(valid_duration_secs)).as_millis() as i64;

    let code = totp.generate_current().unwrap();
    let code = format!("{} {}", &code[0..code.len() / 2], &code[code.len() / 2..]);

    Code {
        unique_idx,
        name: totp.account_name.clone().into(),
        issuer: totp.issuer.clone().unwrap_or_default().into(),
        code: code.into(),
        step: Duration::from_secs(totp.step).as_millis() as i64,
        start_unix_time: unix_time.as_millis() as i64,
        valid_until_unix_time,
    }
}
