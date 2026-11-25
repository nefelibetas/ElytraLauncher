//! Windows GUI 宿主专用：为子进程继承准备一个隐藏控制台。

/// 为当前进程分配一个**隐藏的**控制台（仅当当前没有控制台时）。第三方库内部 spawn
/// （easytier 的 `cmd /C ver`、`ipconfig`、`ping`、`nslookup`）无法装配 `CREATE_NO_WINDOW`，
/// 本进程持有后它们继承、不再新建窗口。幂等且保守：已有控制台直接返回，**绝不隐藏用户终端窗口**。
pub fn ensure_hidden_console() {
    #[cfg(target_os = "windows")]
    {
        #[link(name = "Kernel32")]
        extern "system" {
            fn GetConsoleWindow() -> *mut std::ffi::c_void;
            fn AllocConsole() -> i32;
        }
        #[link(name = "User32")]
        extern "system" {
            fn ShowWindow(hwnd: *mut std::ffi::c_void, cmd_show: i32) -> i32;
        }
        const SW_HIDE: i32 = 0;
        // SAFETY: 只读写本进程控制台状态，无指针解引用；失败返回空句柄/0，静默忽略。
        unsafe {
            if !GetConsoleWindow().is_null() {
                return;
            }
            if AllocConsole() == 0 {
                return;
            }
            ShowWindow(GetConsoleWindow(), SW_HIDE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_console_bootstrap_is_idempotent() {
        ensure_hidden_console();
        ensure_hidden_console();
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn hidden_console_keeps_existing_console() {
        #[link(name = "Kernel32")]
        extern "system" {
            fn GetConsoleWindow() -> *mut std::ffi::c_void;
        }
        let before = unsafe { GetConsoleWindow() };
        ensure_hidden_console();
        let after = unsafe { GetConsoleWindow() };
        if !before.is_null() {
            assert_eq!(before, after, "已有控制台时不得重新分配或隐藏");
        }
    }
}
