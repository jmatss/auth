use crate::{Code, MainWindow};
use slint::{Model, ModelRc, SharedString, VecModel, Weak};
use std::time::Duration;

pub fn load_codes() -> ModelRc<Code> {
    VecModel::from_slice(&[Code {
        code: "951 565".into(),
        expire_countdown: Duration::from_secs(30).as_millis() as i64,
        id: "0".into(),
        name: "SCANIA".into(),
    }])
}

pub fn add_code(main_window: &Weak<MainWindow>, name: SharedString, secret: SharedString) {
    main_window
        .upgrade()
        .unwrap()
        .get_codes()
        .as_any()
        .downcast_ref::<VecModel<Code>>()
        .unwrap()
        .push(Code {
            code: "123 456".into(),
            expire_countdown: Duration::from_secs(30).as_millis() as i64,
            id: "X".into(),
            name: name,
        });
}
