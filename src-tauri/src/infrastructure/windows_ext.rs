use windows::Win32::Foundation::{HWND, RECT, POINT};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, GetWindowRect,
    ShowWindow, SetForegroundWindow, IsWindowVisible, IsIconic,
    SW_RESTORE, SW_SHOWNA, GetGUIThreadInfo, GUITHREADINFO,
    SetWindowPos, HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SWP_NOACTIVATE,
    MessageBoxW, MB_ICONERROR, MB_OK
};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, CoCreateInstance, CLSCTX_ALL};
use windows::Win32::System::Ole::{SafeArrayAccessData, SafeArrayUnaccessData, SafeArrayDestroy};
use windows::Win32::UI::Accessibility::{IUIAutomation, CUIAutomation, IUIAutomationTextPattern, UIA_PATTERN_ID};
use windows::core::{Interface, IUnknown};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY
};

/// 安全封装的窗口辅助工具
pub struct WindowExt;

impl WindowExt {
    /// 尝试通过 UIAutomation 获取自绘引擎(如 Chrome/VSCode)的光标位置
    fn get_uia_caret_pos() -> Option<(i32, i32, i32)> {
        unsafe {
            // 初始化 COM 库
            let co_init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let need_uninit = co_init.is_ok() || co_init == windows::Win32::Foundation::S_FALSE;
            
            let result = (|| -> windows::core::Result<Option<(i32, i32, i32)>> {
                // 创建 IUIAutomation 实例
                let uia: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_ALL)?;
                
                // 获取当前具有键盘焦点的元素
                let focused_element = uia.GetFocusedElement()?;
                
                // 请求 TextPattern 接口
                let pattern_id = UIA_PATTERN_ID(10014); // UIA_TextPatternId is 10014
                let pattern: IUnknown = focused_element.GetCurrentPattern(pattern_id)?;
                let text_pattern: IUIAutomationTextPattern = pattern.cast()?;
                
                // 获取当前选区数组（对于光标，通常是长度为1的选区）
                let selection = text_pattern.GetSelection()?;
                
                if selection.Length()? > 0 {
                    let range = selection.GetElement(0)?;
                    
                    // 获取边界矩形数组 SAFEARRAY of doubles [x, y, width, height, x2, y2, ...]
                    let rects = range.GetBoundingRectangles()?;
                    if !rects.is_null() {
                        let lbound = windows::Win32::System::Ole::SafeArrayGetLBound(rects, 1).unwrap_or(0);
                        let ubound = windows::Win32::System::Ole::SafeArrayGetUBound(rects, 1).unwrap_or(-1);
                        let count = ubound - lbound + 1;

                        if count >= 4 {
                            let mut data_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
                            if SafeArrayAccessData(rects, &mut data_ptr).is_ok() && !data_ptr.is_null() {
                                let f64_slice = std::slice::from_raw_parts(data_ptr as *const f64, count as usize);
                                let x = f64_slice[0];
                                let y = f64_slice[1];
                                let _width = f64_slice[2];
                                let height = f64_slice[3];
                                
                                let _ = SafeArrayUnaccessData(rects);
                                let _ = SafeArrayDestroy(rects);
                                
                                if x.is_finite() && y.is_finite() && height.is_finite() {
                                    let x_i32 = x as i32;
                                    let y_i32 = y as i32;
                                    let h_i32 = height as i32;
                                    
                                    // 检查坐标是否在合理范围内，防止 UIA 返回异常大数值导致后续计算溢出
                                    if x_i32 > -32000 && x_i32 < 32000 && y_i32 > -32000 && y_i32 < 32000 {
                                        return Ok(Some((x_i32, y_i32.saturating_add(h_i32), y_i32)));
                                    }
                                }
                            }
                        }
                        let _ = SafeArrayDestroy(rects);
                    }
                }
                
                Ok(None)
            })();

            let pos = result.unwrap_or(None);
            
            if need_uninit {
                CoUninitialize();
            }
            
            pos
        }
    }

    /// 获取当前活动窗口的输入光标(Caret)屏幕坐标
    pub fn get_caret_pos() -> Option<(i32, i32, i32)> {
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.0.is_null() { return None; }
            let thread_id = GetWindowThreadProcessId(hwnd, None);
            let mut gui_info = GUITHREADINFO::default();
            gui_info.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
            
            // 第一梯队：传统 GDI 获取
            if GetGUIThreadInfo(thread_id, &mut gui_info).is_ok() {
                if !gui_info.hwndCaret.0.is_null() {
                    let mut pt_bottom = POINT { x: gui_info.rcCaret.left, y: gui_info.rcCaret.bottom };
                    let mut pt_top = POINT { x: gui_info.rcCaret.left, y: gui_info.rcCaret.top };
                    if windows::Win32::Graphics::Gdi::ClientToScreen(gui_info.hwndCaret, &mut pt_bottom).as_bool() &&
                       windows::Win32::Graphics::Gdi::ClientToScreen(gui_info.hwndCaret, &mut pt_top).as_bool() {
                        return Some((pt_bottom.x, pt_bottom.y, pt_top.y));
                    }
                }
            }
            
            // 第二梯队：UIA 辅助功能获取（解决 Chrome/VSCode 等自绘光标）
            if let Some(pos) = Self::get_uia_caret_pos() {
                return Some(pos);
            }

            None
        }
    }

    /// 获取当前前台窗口句柄
    pub fn get_foreground_window() -> HWND {
        unsafe { GetForegroundWindow() }
    }

    /// 检查窗口是否可见
    pub fn is_window_visible(hwnd: HWND) -> bool {
        unsafe { IsWindowVisible(hwnd).as_bool() }
    }

    /// 获取窗口矩形区域
    pub fn get_window_rect(hwnd: HWND) -> Option<RECT> {
        let mut rect = RECT::default();
        unsafe {
            if GetWindowRect(hwnd, &mut rect).is_ok() {
                Some(rect)
            } else {
                None
            }
        }
    }

    /// 释放 Windows 键（通过发送哑键 0xFF 拦截并取消 Windows 开始菜单弹出）
    pub fn release_win_keys() {
        unsafe {
            let dummy_vk = VIRTUAL_KEY(0xFF);
            let inputs = [
                Self::create_key_input(dummy_vk, false),
                Self::create_key_input(dummy_vk, true),
            ];
            SendInput(&inputs, core::mem::size_of::<INPUT>() as i32);
        }
    }

    /// Restore focus without sharing an input queue with another application.
    pub fn force_focus_window(hwnd: HWND) {
        if hwnd.0.is_null() { return; }
        
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() { return; }
            let should_restore = IsIconic(hwnd).as_bool();

            if should_restore {
                let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindowAsync(hwnd, SW_RESTORE);
            }
            let _ = SetForegroundWindow(hwnd);
        }
    }

    /// 无感显示置顶窗口（不夺取焦点）
    pub fn show_window_no_activate(hwnd: HWND) {
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNA);
            let _ = SetWindowPos(
                hwnd, 
                Some(HWND_TOPMOST), 
                0, 0, 0, 0, 
                SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW | SWP_NOACTIVATE
            );
        }
    }

    /// 无激活显示普通窗口（不置顶）
    pub fn show_window_no_activate_normal(hwnd: HWND) {
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNA);
            // Bring to front without activation by temporarily toggling TOPMOST.
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                0, 0, 0, 0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW | SWP_NOACTIVATE
            );
            let _ = SetWindowPos(
                hwnd,
                None,
                0, 0, 0, 0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW | SWP_NOACTIVATE
            );
        }
    }

    /// 等待指定窗口获得焦点 (使用 usize 避免 HWND 的非 Send 约束)
    pub async fn wait_for_focus_raw(target_ptr: usize, timeout_ms: u64) -> bool {
        let start = std::time::Instant::now();
        while start.elapsed().as_millis() < timeout_ms as u128 {
            if unsafe { GetForegroundWindow().0 as usize == target_ptr } {
                return true;
            }
            tokio::task::yield_now().await;
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        unsafe { GetForegroundWindow().0 as usize == target_ptr }
    }

    /// 弹出错误消息框
    pub fn show_error_box(title: &str, msg: &str) {
        use windows::core::PCWSTR;
        let title_w: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
        let msg_w: Vec<u16> = msg.encode_utf16().chain(std::iter::once(0)).collect();
        
        unsafe {
            let _ = MessageBoxW(
                None,
                PCWSTR(msg_w.as_ptr()),
                PCWSTR(title_w.as_ptr()),
                MB_ICONERROR | MB_OK,
            );
        }
    }

    fn create_key_input(vk: VIRTUAL_KEY, is_up: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    dwFlags: if is_up { KEYEVENTF_KEYUP } else { windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS(0) },
                    ..Default::default()
                }
            }
        }
    }
}
