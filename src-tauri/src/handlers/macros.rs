#[cfg(all(feature = "gui", not(feature = "headless")))]
#[macro_export]
macro_rules! state_type {
    () => {
        tauri::State<'_, State>
    };
}

#[cfg(feature = "headless")]
#[macro_export]
macro_rules! state_type {
    () => {
        State
    };
}
