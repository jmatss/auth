use std::{
    rc::Rc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use android_activity::AndroidApp;
use i_slint_core::animations::Instant;
use jni::JavaVM;
use slint::Model;
use tokio::{sync::mpsc::UnboundedReceiver, time::timeout};
use totp_rs::{Rfc6238Error, TOTP, TotpUrlError};

use crate::{AppState, Code, MoveDirection, java::JavaHelpers};

pub enum CodeMessage {
    /// The `String` is the URL of the added code.
    Add(String),
    /// The `i32` is the `unique_idx` of the code to remove.
    Remove(i32),
    /// The `i32` is the `unique_idx` of the code. The first `String` is the new name
    /// and the last `String` is the new issuer.
    Edit(i32, String, String),
    /// The `i32` is the `unique_idx` of the code. The `MoveDirection` is which direction
    /// the code should be moved in the "list of codes".
    Move(i32, MoveDirection),
}

pub async fn code_handler(state: Rc<AppState>, mut reciver: UnboundedReceiver<CodeMessage>) {
    let mut unique_idx = 0;

    // Used to prevent adding multiple of the same QR code when it is added. Since the adding
    // operation is async, the Sender might send multiple `CodeMessage::Add` to this `code_handler`
    // before the code is actually added. So need to prevent accidentally adding the same code
    // multiple times.
    let mut last_add_time = Instant::now();
    let debounce_time = Duration::from_secs(1);

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
                state
                    .codes
                    .set_row_data(row, totp_to_code(code.unique_idx, totp));
            }
        }

        match timeout(Duration::from_millis(500), reciver.recv()).await {
            Ok(Some(CodeMessage::Add(url))) => {
                if last_add_time + debounce_time < Instant::now() {
                    let was_added = handle_add(&state, &mut totps, &url, unique_idx);
                    if was_added {
                        unique_idx += 1;
                        last_add_time = Instant::now();
                    }
                }
            }
            Ok(Some(CodeMessage::Remove(unique_idx))) => {
                handle_remove(&state, &mut totps, unique_idx);
            }
            Ok(Some(CodeMessage::Edit(unique_idx, new_name, new_issuer))) => {
                handle_edit(&state, &mut totps, unique_idx, new_name, new_issuer);
            }
            Ok(Some(CodeMessage::Move(unique_idx, direction))) => {
                handle_move(&state, &mut totps, unique_idx, direction);
            }
            // Timeout. This is expected, continue loop as normal.
            Err(_) => (),
            // The channel is closed. Break out of inifinite loop.
            Ok(None) => break,
        }
    }
}

fn handle_add(state: &Rc<AppState>, totps: &mut Vec<TOTP>, url: &str, unique_idx: i32) -> bool {
    let vm = unsafe { JavaVM::from_raw(state.app.vm_as_ptr() as *mut _).unwrap() };
    let mut env = vm.attach_current_thread().unwrap();

    match url_to_totp(&url) {
        Ok(totp) => {
            let code = totp_to_code(unique_idx, &totp);
            let normalized_url = totp.get_url();

            let already_exists = totps.iter().any(|t| t.get_url() == normalized_url);
            if already_exists {
                state.java_helpers.show_error(
                    &mut env,
                    "Error",
                    "This TOTP already exists in the application",
                );

                false
            } else {
                state
                    .java_helpers
                    .write_url_to_disk(&mut env, &normalized_url);

                totps.push(totp);
                state.codes.push(code);

                true
            }
        }
        Err(err) => {
            state
                .java_helpers
                .show_error(&mut env, "Error", &err.to_string());

            false
        }
    }
}

fn handle_remove(state: &Rc<AppState>, totps: &mut Vec<TOTP>, unique_idx: i32) {
    let vm = unsafe { JavaVM::from_raw(state.app.vm_as_ptr() as *mut _).unwrap() };
    let mut env = vm.attach_current_thread().unwrap();

    let row = state
        .codes
        .iter()
        .enumerate()
        .find(|(_, c)| c.unique_idx == unique_idx)
        .unwrap()
        .0;

    let totp = totps.remove(row);
    let normalized_url = totp.get_url();
    state.codes.remove(row);

    state
        .java_helpers
        .remove_url_from_disk(&mut env, &normalized_url);
}

fn handle_edit(
    state: &Rc<AppState>,
    totps: &mut Vec<TOTP>,
    unique_idx: i32,
    new_name: String,
    new_issuer: String,
) {
    let vm = unsafe { JavaVM::from_raw(state.app.vm_as_ptr() as *mut _).unwrap() };
    let mut env = vm.attach_current_thread().unwrap();

    let row = state
        .codes
        .iter()
        .enumerate()
        .find(|(_, c)| c.unique_idx == unique_idx)
        .unwrap()
        .0;

    let totp = &mut totps[row];
    let old_normalized_url = totp.get_url();

    totp.account_name = new_name;
    totp.issuer = if new_issuer.is_empty() {
        None
    } else {
        Some(new_issuer)
    };

    let new_normalized_url = totp.get_url();
    state
        .java_helpers
        .edit_url_on_disk(&mut env, &old_normalized_url, &new_normalized_url);

    state
        .codes
        .set_row_data(row, totp_to_code(unique_idx, totp));
}

fn handle_move(
    state: &Rc<AppState>,
    totps: &mut Vec<TOTP>,
    unique_idx: i32,
    direction: MoveDirection,
) -> bool {
    let vm = unsafe { JavaVM::from_raw(state.app.vm_as_ptr() as *mut _).unwrap() };
    let mut env = vm.attach_current_thread().unwrap();

    let row = state
        .codes
        .iter()
        .enumerate()
        .find(|(_, c)| c.unique_idx == unique_idx)
        .unwrap()
        .0;

    let row_count = state.codes.row_count();
    let other_row = match direction {
        MoveDirection::Up if row == 0 => {
            state.java_helpers.show_error(
                &mut env,
                "Info",
                "Unable to move upwards since it is already at the top",
            );

            return false;
        }
        MoveDirection::Down if row + 1 == row_count => {
            state.java_helpers.show_error(
                &mut env,
                "Info",
                "Unable to move downwards since it is already at the bottom",
            );

            return false;
        }

        MoveDirection::Up => row - 1,
        MoveDirection::Down => row + 1,
    };

    let first_url = totps[row].get_url();
    let second_url = totps[other_row].get_url();

    totps.swap(row, other_row);
    state.codes.swap(row, other_row);

    state
        .java_helpers
        .swap_urls_on_disk(&mut env, &first_url, &second_url);

    true
}

fn load_totps(app: AndroidApp, java_helpers: &JavaHelpers) -> Vec<TOTP> {
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr() as *mut _).unwrap() };
    let mut env = vm.attach_current_thread().unwrap();

    java_helpers
        .get_urls_from_disk(&mut env)
        .iter()
        .map(|url| url_to_totp(url).unwrap())
        .collect::<Vec<_>>()
}

fn totp_to_code(unique_idx: i32, totp: &TOTP) -> Code {
    let unix_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();

    let valid_duration_secs = totp.step - unix_time.as_secs() % totp.step;
    let valid_until_unix_time =
        (unix_time + Duration::from_secs(valid_duration_secs)).as_millis() as i64;

    let c = totp.generate_current().unwrap();
    let code = format!("{} {}", &c[0..c.len() / 2], &c[c.len() / 2..]);
    let nc = totp.generate(totp.next_step_current().unwrap());
    let next_code = format!("{} {}", &nc[0..nc.len() / 2], &nc[nc.len() / 2..]);

    Code {
        unique_idx,
        name: totp.account_name.clone().into(),
        issuer: totp.issuer.clone().unwrap_or_default().into(),
        code: code.into(),
        next_code: next_code.into(),
        step: Duration::from_secs(totp.step).as_millis() as i64,
        start_unix_time: unix_time.as_millis() as i64,
        valid_until_unix_time,
    }
}

/// `TOTP::from_url(..)` enforces that the secret is at least 128 bits.
/// But some implementations have secrets less that 128 bits (80 bits is common).
///
/// This function does the exact same thing as `TOTP::from_url(..)`, but changes the
/// secrets minimum length from 128 bits to 80 bits.
/// Logic copied from `TOTP::new(..)` code.
/// [https://github.com/constantoine/totp-rs/blob/v5.7.0/src/lib.rs#L503]
/// [https://github.com/constantoine/totp-rs/issues/46]
fn url_to_totp(url: &str) -> Result<TOTP, TotpUrlError> {
    let totp = TOTP::from_url_unchecked(url).unwrap();
    assert_digits(&totp.digits)?;
    assert_secret_length(&totp.secret)?;
    if totp.issuer.is_some() && totp.issuer.as_ref().unwrap().contains(':') {
        Err(TotpUrlError::Issuer(
            totp.issuer.as_ref().unwrap().to_string(),
        ))
    } else if totp.account_name.contains(':') {
        Err(TotpUrlError::AccountName(totp.account_name))
    } else {
        Ok(totp)
    }
}

/// Copy paste of private function in `totp_rs::rfc`.
/// [https://github.com/constantoine/totp-rs/blob/v5.7.0/src/rfc.rs#L38]
pub fn assert_digits(digits: &usize) -> Result<(), Rfc6238Error> {
    if !(&6..=&8).contains(&digits) {
        Err(Rfc6238Error::InvalidDigits(*digits))
    } else {
        Ok(())
    }
}

/// Copy paste of private function in `totp_rs::rfc`.
/// [https://github.com/constantoine/totp-rs/blob/v5.7.0/src/rfc.rs#L48]
pub fn assert_secret_length(secret: &[u8]) -> Result<(), Rfc6238Error> {
    if secret.as_ref().len() < 10 {
        Err(Rfc6238Error::SecretTooSmall(secret.as_ref().len() * 8))
    } else {
        Ok(())
    }
}
