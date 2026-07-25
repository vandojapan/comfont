use std::{ffi::c_void, mem::size_of, path::PathBuf, sync::OnceLock};

use compositefont_core::ProfileDocument;
use windows::{
    Win32::{
        Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, SIZE, WPARAM},
        Graphics::Gdi::{
            BeginPaint, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, COLOR_WINDOW, CreateFontW,
            CreatePen, DEFAULT_CHARSET, DEFAULT_GUI_FONT, DEFAULT_PITCH, DeleteObject, EndPaint,
            EnumFontFamiliesExW, FIXED, FW_NORMAL, GDI_ERROR, GGO_METRICS, GLYPHMETRICS, GetDC,
            GetGlyphOutlineW, GetStockObject, GetTextExtentPoint32W, GetTextMetricsW, HDC, HGDIOBJ,
            InvalidateRect, LOGFONTW, LineTo, MAT2, MoveToEx, OUT_DEFAULT_PRECIS, PAINTSTRUCT,
            PS_SOLID, Rectangle, ReleaseDC, SelectObject, SetBkMode, TEXTMETRICW, TRANSPARENT,
            TextOutW,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Controls::{
                BST_CHECKED, CB_SETMINVISIBLE, ICC_LISTVIEW_CLASSES, INITCOMMONCONTROLSEX,
                InitCommonControlsEx, LIST_VIEW_ITEM_STATE_FLAGS, LVCF_TEXT, LVCF_WIDTH, LVCOLUMNW,
                LVIF_TEXT, LVIS_FOCUSED, LVIS_SELECTED, LVITEMW, LVM_DELETEALLITEMS,
                LVM_INSERTCOLUMNW, LVM_INSERTITEMW, LVM_SETEXTENDEDLISTVIEWSTYLE, LVM_SETITEMSTATE,
                LVM_SETITEMTEXTW, LVN_ITEMCHANGED, LVS_EX_DOUBLEBUFFER, LVS_EX_FULLROWSELECT,
                LVS_EX_GRIDLINES, LVS_REPORT, LVS_SHOWSELALWAYS, LVS_SINGLESEL, NMLISTVIEW,
                WC_LISTVIEWW,
            },
            Input::KeyboardAndMouse::EnableWindow,
            WindowsAndMessaging::{
                BM_GETCHECK, BN_CLICKED, BS_AUTOCHECKBOX, BS_DEFPUSHBUTTON, BS_GROUPBOX,
                CB_ADDSTRING, CB_GETCURSEL, CB_RESETCONTENT, CB_SETCURSEL, CBN_SELCHANGE,
                CBS_AUTOHSCROLL, CBS_DROPDOWN, CBS_DROPDOWNLIST, CBS_NOINTEGRALHEIGHT,
                CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW,
                DefWindowProcW, DestroyWindow, DispatchMessageW, ES_AUTOHSCROLL, GetMessageW,
                GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, HMENU, IDC_ARROW,
                IsDialogMessageW, IsWindow, LoadCursorW, MB_ICONERROR, MB_ICONINFORMATION, MB_OK,
                MSG, MessageBoxW, RegisterClassW, SW_SHOW, SendMessageW, SetForegroundWindow,
                SetWindowLongPtrW, SetWindowTextW, ShowWindow, TranslateMessage, WINDOW_EX_STYLE,
                WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_NCCREATE, WM_NOTIFY, WM_PAINT,
                WM_SETFONT, WNDCLASSW, WS_BORDER, WS_CAPTION, WS_CHILD, WS_EX_CLIENTEDGE,
                WS_EX_CONTROLPARENT, WS_EX_DLGMODALFRAME, WS_GROUP, WS_OVERLAPPED, WS_SYSMENU,
                WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
            },
        },
    },
    core::{PCWSTR, PWSTR, w},
};

use crate::{
    model::{CATEGORY_ROWS, EditorModel},
    storage::save_document,
};

const CLASS_NAME: PCWSTR = w!("CompositeFontEditorWindow");
const WINDOW_WIDTH: i32 = 800;
const WINDOW_HEIGHT: i32 = 790;

const ID_PROFILE: usize = 100;
const ID_UNIT: usize = 101;
const ID_LIST: usize = 110;
const ID_SELECTED_CATEGORY: usize = 111;
const ID_FONT: usize = 120;
const ID_SIZE: usize = 121;
const ID_BASELINE: usize = 122;
const ID_TRACKING: usize = 123;
const ID_APPLY: usize = 124;
const ID_NEW: usize = 130;
const ID_SAVE: usize = 131;
const ID_DELETE: usize = 132;
const ID_SAMPLE_VISIBLE: usize = 140;
const ID_SPECIAL: usize = 141;
const ID_OK: usize = 1;
const ID_CANCEL: usize = 2;

#[derive(Clone, Copy, Default)]
struct Controls {
    profile: HWND,
    list: HWND,
    selected_category: HWND,
    font: HWND,
    size: HWND,
    baseline: HWND,
    tracking: HWND,
    sample_visible: HWND,
}

struct DialogContext {
    model: EditorModel,
    persisted_document: ProfileDocument,
    profile_path: PathBuf,
    controls: Controls,
    refreshing: bool,
    sample_visible: bool,
}

impl DialogContext {
    fn new(document: ProfileDocument, profile_path: PathBuf) -> Self {
        Self {
            model: EditorModel::new(document.clone()),
            persisted_document: document,
            profile_path,
            controls: Controls::default(),
            refreshing: false,
            sample_visible: true,
        }
    }
}

pub fn show_editor(
    owner: aviutl2::Win32WindowHandle,
    document: ProfileDocument,
    profile_path: PathBuf,
) -> Result<ProfileDocument, String> {
    register_window_class()?;
    let owner = HWND(owner.hwnd.get() as *mut c_void);
    let mut context = Box::new(DialogContext::new(document, profile_path));
    let context_ptr = (&mut *context) as *mut DialogContext;
    let instance = module_instance()?;

    let window = unsafe {
        CreateWindowExW(
            WS_EX_DLGMODALFRAME | WS_EX_CONTROLPARENT,
            CLASS_NAME,
            w!("合成フォント"),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            WINDOW_WIDTH,
            WINDOW_HEIGHT,
            Some(owner),
            None,
            Some(instance),
            Some(context_ptr.cast()),
        )
        .map_err(|error| format!("合成フォント画面を作成できません: {error}"))?
    };

    unsafe {
        let _ = EnableWindow(owner, false);
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetForegroundWindow(window);

        let mut message = MSG::default();
        while IsWindow(Some(window)).as_bool() {
            let status = GetMessageW(&mut message, None, 0, 0).0;
            if status <= 0 {
                break;
            }
            if !IsDialogMessageW(window, &message).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }

        let _ = EnableWindow(owner, true);
        let _ = SetForegroundWindow(owner);
    }

    Ok(context.persisted_document)
}

fn register_window_class() -> Result<(), String> {
    static REGISTRATION: OnceLock<Result<(), String>> = OnceLock::new();
    REGISTRATION
        .get_or_init(|| unsafe {
            let instance = module_instance()?;
            let class = WNDCLASSW {
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(window_proc),
                hInstance: instance,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                hbrBackground: windows::Win32::Graphics::Gdi::GetSysColorBrush(COLOR_WINDOW),
                lpszClassName: CLASS_NAME,
                ..Default::default()
            };
            if RegisterClassW(&class) == 0 {
                return Err(format!(
                    "合成フォント画面のウィンドウクラスを登録できません: {}",
                    std::io::Error::last_os_error()
                ));
            }
            Ok(())
        })
        .clone()
}

fn module_instance() -> Result<HINSTANCE, String> {
    unsafe {
        GetModuleHandleW(None)
            .map(|module| HINSTANCE(module.0))
            .map_err(|error| format!("モジュールハンドルを取得できません: {error}"))
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = unsafe { &*(lparam.0 as *const CREATESTRUCTW) };
        unsafe {
            SetWindowLongPtrW(
                window,
                windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
                create.lpCreateParams as isize,
            );
        }
    }

    let context_ptr = unsafe {
        GetWindowLongPtrW(
            window,
            windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
        ) as *mut DialogContext
    };
    if context_ptr.is_null() {
        return unsafe { DefWindowProcW(window, message, wparam, lparam) };
    }
    let context = unsafe { &mut *context_ptr };

    match message {
        WM_CREATE => match unsafe { create_controls(window, context) } {
            Ok(()) => LRESULT(0),
            Err(error) => {
                unsafe { show_error(Some(window), &error) };
                LRESULT(-1)
            }
        },
        WM_COMMAND => {
            let id = wparam.0 & 0xffff;
            let notification = ((wparam.0 >> 16) & 0xffff) as u32;
            unsafe { handle_command(window, context, id, notification) };
            LRESULT(0)
        }
        WM_NOTIFY => {
            if lparam.0 != 0 {
                let notification = unsafe { &*(lparam.0 as *const NMLISTVIEW) };
                let selected_row_changed = notification.hdr.idFrom == ID_LIST
                    && notification.hdr.code == LVN_ITEMCHANGED
                    && notification.iItem >= 0
                    && notification.uNewState & LVIS_SELECTED.0 != 0
                    && !context.refreshing;
                if selected_row_changed {
                    match unsafe { apply_editor_fields(context) } {
                        Ok(()) => {
                            if context.model.select_category(notification.iItem as usize) {
                                unsafe { refresh_editor_fields(window, context) };
                            }
                        }
                        Err(error) => unsafe { show_error(Some(window), &error) },
                    }
                }
            }
            LRESULT(0)
        }
        WM_PAINT => {
            unsafe { paint_preview(window, context) };
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = unsafe { DestroyWindow(window) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

unsafe fn create_controls(window: HWND, context: &mut DialogContext) -> Result<(), String> {
    unsafe {
        let _ = InitCommonControlsEx(&INITCOMMONCONTROLSEX {
            dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_LISTVIEW_CLASSES,
        });
    }
    let instance = module_instance()?;
    let font = unsafe { GetStockObject(DEFAULT_GUI_FONT) };

    unsafe {
        create_label(window, instance, "合成フォント：", 20, 20, 95, 24, 0, font)?;
        context.controls.profile = create_child(
            window,
            instance,
            w!("COMBOBOX"),
            "",
            WS_CHILD
                | WS_VISIBLE
                | WS_TABSTOP
                | WS_VSCROLL
                | WINDOW_STYLE((CBS_DROPDOWN | CBS_AUTOHSCROLL | CBS_NOINTEGRALHEIGHT) as u32),
            WINDOW_EX_STYLE(0),
            110,
            16,
            340,
            300,
            ID_PROFILE,
            font,
        )?;
        create_label(window, instance, "単位：", 590, 20, 55, 24, 0, font)?;
        let unit = create_child(
            window,
            instance,
            w!("COMBOBOX"),
            "",
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(CBS_DROPDOWNLIST as u32),
            WINDOW_EX_STYLE(0),
            645,
            16,
            105,
            100,
            ID_UNIT,
            font,
        )?;
        combo_add(unit, "%");
        SendMessageW(unit, CB_SETCURSEL, Some(WPARAM(0)), None);

        context.controls.list = create_child(
            window,
            instance,
            WC_LISTVIEWW,
            "",
            WS_CHILD
                | WS_VISIBLE
                | WS_TABSTOP
                | WS_BORDER
                | WINDOW_STYLE(LVS_REPORT | LVS_SINGLESEL | LVS_SHOWSELALWAYS),
            WS_EX_CLIENTEDGE,
            20,
            55,
            730,
            220,
            ID_LIST,
            font,
        )?;
        SendMessageW(
            context.controls.list,
            LVM_SETEXTENDEDLISTVIEWSTYLE,
            Some(WPARAM(
                (LVS_EX_FULLROWSELECT | LVS_EX_GRIDLINES | LVS_EX_DOUBLEBUFFER) as usize,
            )),
            Some(LPARAM(
                (LVS_EX_FULLROWSELECT | LVS_EX_GRIDLINES | LVS_EX_DOUBLEBUFFER) as isize,
            )),
        );
        for (index, (title, width)) in [
            ("文字種", 105),
            ("フォント", 280),
            ("サイズ", 95),
            ("ベース", 95),
            ("字送り", 95),
        ]
        .into_iter()
        .enumerate()
        {
            list_insert_column(context.controls.list, index, title, width);
        }

        create_child(
            window,
            instance,
            w!("BUTTON"),
            "選択行の設定",
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_GROUPBOX as u32),
            WINDOW_EX_STYLE(0),
            20,
            285,
            730,
            105,
            0,
            font,
        )?;
        context.controls.selected_category = create_label(
            window,
            instance,
            "漢字",
            35,
            310,
            75,
            24,
            ID_SELECTED_CATEGORY,
            font,
        )?;
        create_label(window, instance, "フォント", 115, 297, 80, 20, 0, font)?;
        context.controls.font = create_child(
            window,
            instance,
            w!("COMBOBOX"),
            "",
            WS_CHILD
                | WS_VISIBLE
                | WS_TABSTOP
                | WINDOW_STYLE((CBS_DROPDOWN | CBS_AUTOHSCROLL) as u32),
            WINDOW_EX_STYLE(0),
            110,
            318,
            290,
            220,
            ID_FONT,
            font,
        )?;
        create_label(window, instance, "サイズ", 415, 297, 70, 20, 0, font)?;
        context.controls.size = create_edit(window, instance, 410, 318, 80, ID_SIZE, font)?;
        create_label(window, instance, "ベース", 500, 297, 70, 20, 0, font)?;
        context.controls.baseline = create_edit(window, instance, 495, 318, 80, ID_BASELINE, font)?;
        create_label(window, instance, "字送り", 585, 297, 70, 20, 0, font)?;
        context.controls.tracking = create_edit(window, instance, 580, 318, 80, ID_TRACKING, font)?;
        create_child(
            window,
            instance,
            w!("BUTTON"),
            "適用",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            WINDOW_EX_STYLE(0),
            670,
            317,
            65,
            26,
            ID_APPLY,
            font,
        )?;
        create_label(
            window,
            instance,
            "各数値は%で入力（サイズ100 = 等倍）",
            110,
            353,
            350,
            20,
            0,
            font,
        )?;

        for (text, x, width, id) in [
            ("新規…", 20, 100, ID_NEW),
            ("保存", 130, 100, ID_SAVE),
            ("フォントを削除", 240, 140, ID_DELETE),
            ("特殊文字…", 630, 120, ID_SPECIAL),
        ] {
            create_child(
                window,
                instance,
                w!("BUTTON"),
                text,
                WS_CHILD | WS_VISIBLE | WS_TABSTOP,
                WINDOW_EX_STYLE(0),
                x,
                400,
                width,
                27,
                id,
                font,
            )?;
        }

        context.controls.sample_visible = create_child(
            window,
            instance,
            w!("BUTTON"),
            "サンプルを表示",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_AUTOCHECKBOX as u32),
            WINDOW_EX_STYLE(0),
            20,
            442,
            150,
            24,
            ID_SAMPLE_VISIBLE,
            font,
        )?;
        SendMessageW(
            context.controls.sample_visible,
            windows::Win32::UI::WindowsAndMessaging::BM_SETCHECK,
            Some(WPARAM(BST_CHECKED.0 as usize)),
            None,
        );
        create_label(
            window,
            instance,
            "選択したプロファイルの文字種別プレビュー",
            180,
            444,
            360,
            22,
            0,
            font,
        )?;

        create_child(
            window,
            instance,
            w!("BUTTON"),
            "OK",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_GROUP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32),
            WINDOW_EX_STYLE(0),
            525,
            685,
            105,
            30,
            ID_OK,
            font,
        )?;
        create_child(
            window,
            instance,
            w!("BUTTON"),
            "キャンセル",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            WINDOW_EX_STYLE(0),
            645,
            685,
            105,
            30,
            ID_CANCEL,
            font,
        )?;
    }

    unsafe {
        fill_font_combo(context);
        SendMessageW(
            context.controls.font,
            CB_SETMINVISIBLE,
            Some(WPARAM(12)),
            None,
        );
        refresh_all(window, context);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
unsafe fn create_child(
    parent: HWND,
    instance: HINSTANCE,
    class: PCWSTR,
    text: &str,
    style: WINDOW_STYLE,
    ex_style: WINDOW_EX_STYLE,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    id: usize,
    font: HGDIOBJ,
) -> Result<HWND, String> {
    let text = wide(text);
    let child = unsafe {
        CreateWindowExW(
            ex_style,
            class,
            PCWSTR(text.as_ptr()),
            style,
            x,
            y,
            width,
            height,
            Some(parent),
            Some(HMENU(id as *mut c_void)),
            Some(instance),
            None,
        )
        .map_err(|error| format!("画面部品を作成できません: {error}"))?
    };
    unsafe {
        SendMessageW(
            child,
            WM_SETFONT,
            Some(WPARAM(font.0 as usize)),
            Some(LPARAM(1)),
        );
    }
    Ok(child)
}

#[allow(clippy::too_many_arguments)]
unsafe fn create_label(
    parent: HWND,
    instance: HINSTANCE,
    text: &str,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    id: usize,
    font: HGDIOBJ,
) -> Result<HWND, String> {
    unsafe {
        create_child(
            parent,
            instance,
            w!("STATIC"),
            text,
            WS_CHILD | WS_VISIBLE,
            WINDOW_EX_STYLE(0),
            x,
            y,
            width,
            height,
            id,
            font,
        )
    }
}

unsafe fn create_edit(
    parent: HWND,
    instance: HINSTANCE,
    x: i32,
    y: i32,
    width: i32,
    id: usize,
    font: HGDIOBJ,
) -> Result<HWND, String> {
    unsafe {
        create_child(
            parent,
            instance,
            w!("EDIT"),
            "",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_BORDER | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
            WS_EX_CLIENTEDGE,
            x,
            y,
            width,
            25,
            id,
            font,
        )
    }
}

unsafe fn handle_command(window: HWND, context: &mut DialogContext, id: usize, notification: u32) {
    match (id, notification) {
        (ID_PROFILE, CBN_SELCHANGE) if !context.refreshing => {
            if let Err(error) = unsafe { apply_editor_fields(context) } {
                unsafe { show_error(Some(window), &error) };
                return;
            }
            let index = unsafe {
                SendMessageW(context.controls.profile, CB_GETCURSEL, None, None).0 as usize
            };
            if context.model.select_profile(index) {
                unsafe { refresh_all(window, context) };
            }
        }
        (ID_APPLY, BN_CLICKED) => match unsafe { apply_editor_fields(context) } {
            Ok(()) => unsafe { refresh_all(window, context) },
            Err(error) => unsafe { show_error(Some(window), &error) },
        },
        (ID_NEW, BN_CLICKED) => {
            if let Err(error) = unsafe { apply_editor_fields(context) } {
                unsafe { show_error(Some(window), &error) };
                return;
            }
            context.model.create_profile();
            unsafe { refresh_all(window, context) };
        }
        (ID_DELETE, BN_CLICKED) => match context.model.delete_selected_profile() {
            Ok(()) => unsafe { refresh_all(window, context) },
            Err(error) => unsafe { show_error(Some(window), &error) },
        },
        (ID_SAVE, BN_CLICKED) => match unsafe { commit_and_save(context) } {
            Ok(()) => unsafe {
                MessageBoxW(
                    Some(window),
                    w!("プロファイルを保存しました。"),
                    w!("合成フォント"),
                    MB_OK | MB_ICONINFORMATION,
                );
                refresh_all(window, context);
            },
            Err(error) => unsafe { show_error(Some(window), &error) },
        },
        (ID_SAMPLE_VISIBLE, BN_CLICKED) => {
            context.sample_visible = unsafe {
                SendMessageW(context.controls.sample_visible, BM_GETCHECK, None, None).0 as u32
                    == BST_CHECKED.0
            };
            let _ = unsafe { InvalidateRect(Some(window), None, true) };
        }
        (ID_SPECIAL, BN_CLICKED) => unsafe {
            MessageBoxW(
                Some(window),
                w!(
                    "共有記号は文脈なしで安定して解決できるよう、ー＝カナ、・＝記号、々＝漢字として扱います。"
                ),
                w!("特殊文字の分類"),
                MB_OK | MB_ICONINFORMATION,
            );
        },
        (ID_OK, BN_CLICKED) => match unsafe { commit_and_save(context) } {
            Ok(()) => {
                let _ = unsafe { DestroyWindow(window) };
            }
            Err(error) => unsafe { show_error(Some(window), &error) },
        },
        (ID_CANCEL, BN_CLICKED) => {
            let _ = unsafe { DestroyWindow(window) };
        }
        _ => {}
    }
}

unsafe fn commit_and_save(context: &mut DialogContext) -> Result<(), String> {
    unsafe { apply_editor_fields(context)? };
    let document = context.model.document().clone();
    save_document(&context.profile_path, &document)?;
    context.persisted_document = document;
    Ok(())
}

unsafe fn apply_editor_fields(context: &mut DialogContext) -> Result<(), String> {
    let profile_name = unsafe { window_text(context.controls.profile) };
    context.model.rename_selected_profile(&profile_name)?;
    let font = unsafe { window_text(context.controls.font) };
    let size = parse_number(&unsafe { window_text(context.controls.size) }, "サイズ")?;
    let baseline = parse_number(
        &unsafe { window_text(context.controls.baseline) },
        "ベースライン",
    )?;
    let tracking = parse_number(&unsafe { window_text(context.controls.tracking) }, "字送り")?;
    context
        .model
        .update_selected_adjustment(font, size, baseline, tracking)
}

fn parse_number(text: &str, label: &str) -> Result<f64, String> {
    let text = text.trim().trim_end_matches('%').trim();
    text.parse::<f64>()
        .map_err(|_| format!("{label}に数値を入力してください。"))
}

unsafe fn refresh_all(window: HWND, context: &mut DialogContext) {
    context.refreshing = true;
    unsafe {
        refresh_profile_combo(context);
        refresh_list(context);
        refresh_editor_fields(window, context);
    }
    context.refreshing = false;
    let _ = unsafe { InvalidateRect(Some(window), None, true) };
}

unsafe fn refresh_profile_combo(context: &DialogContext) {
    unsafe {
        SendMessageW(context.controls.profile, CB_RESETCONTENT, None, None);
        for profile in &context.model.document().profiles {
            combo_add(context.controls.profile, &profile.name);
        }
        SendMessageW(
            context.controls.profile,
            CB_SETCURSEL,
            Some(WPARAM(context.model.selected_profile_index())),
            None,
        );
    }
}

unsafe fn refresh_list(context: &DialogContext) {
    unsafe { SendMessageW(context.controls.list, LVM_DELETEALLITEMS, None, None) };
    let profile = context.model.selected_profile();
    for (row_index, row) in CATEGORY_ROWS.iter().enumerate() {
        let adjustment = profile.adjustment_for(row.class);
        let font = if adjustment.font_family.is_empty() {
            "（変更なし）"
        } else {
            &adjustment.font_family
        };
        unsafe {
            list_insert_row(
                context.controls.list,
                row_index,
                [
                    row.label.to_owned(),
                    font.to_owned(),
                    percent(adjustment.size_ratio),
                    percent(adjustment.baseline_shift_em),
                    percent(adjustment.tracking_adjust_em),
                ],
            );
        }
    }
    let mut item = LVITEMW {
        stateMask: LIST_VIEW_ITEM_STATE_FLAGS(LVIS_SELECTED.0 | LVIS_FOCUSED.0),
        state: LIST_VIEW_ITEM_STATE_FLAGS(LVIS_SELECTED.0 | LVIS_FOCUSED.0),
        ..Default::default()
    };
    unsafe {
        SendMessageW(
            context.controls.list,
            LVM_SETITEMSTATE,
            Some(WPARAM(context.model.selected_category_index())),
            Some(LPARAM((&mut item as *mut LVITEMW) as isize)),
        );
    }
}

unsafe fn refresh_editor_fields(window: HWND, context: &DialogContext) {
    let row = CATEGORY_ROWS[context.model.selected_category_index()];
    let adjustment = context.model.selected_adjustment();
    unsafe {
        set_text(context.controls.selected_category, row.label);
        set_text(context.controls.font, &adjustment.font_family);
        set_text(context.controls.size, &plain_percent(adjustment.size_ratio));
        set_text(
            context.controls.baseline,
            &plain_percent(adjustment.baseline_shift_em),
        );
        set_text(
            context.controls.tracking,
            &plain_percent(adjustment.tracking_adjust_em),
        );
        let _ = InvalidateRect(Some(window), None, true);
    }
}

unsafe fn fill_font_combo(context: &DialogContext) {
    let mut fonts = unsafe { enumerate_system_fonts() };
    for profile in &context.model.document().profiles {
        for row in CATEGORY_ROWS {
            let family = &profile.adjustment_for(row.class).font_family;
            if !family.is_empty() {
                fonts.push(family.clone());
            }
        }
    }
    if fonts.is_empty() {
        fonts.extend(["Yu Gothic UI".to_owned(), "Arial".to_owned()]);
    }
    fonts.sort_unstable();
    fonts.dedup();
    for font in fonts {
        unsafe { combo_add(context.controls.font, &font) };
    }
}

unsafe fn enumerate_system_fonts() -> Vec<String> {
    unsafe extern "system" fn callback(
        logfont: *const LOGFONTW,
        _metric: *const TEXTMETRICW,
        _font_type: u32,
        data: LPARAM,
    ) -> i32 {
        let fonts = unsafe { &mut *(data.0 as *mut Vec<String>) };
        let face_name = unsafe { &(*logfont).lfFaceName };
        let length = face_name
            .iter()
            .position(|character| *character == 0)
            .unwrap_or(face_name.len());
        let name = String::from_utf16_lossy(&face_name[..length]);
        if !name.is_empty() && !name.starts_with('@') {
            fonts.push(name);
        }
        1
    }

    let dc = unsafe { GetDC(None) };
    if dc.is_invalid() {
        return Vec::new();
    }
    let request = LOGFONTW {
        lfCharSet: DEFAULT_CHARSET,
        ..Default::default()
    };
    let mut fonts = Vec::new();
    unsafe {
        EnumFontFamiliesExW(
            dc,
            &request,
            Some(callback),
            LPARAM((&mut fonts as *mut Vec<String>) as isize),
            0,
        );
        ReleaseDC(None, dc);
    }
    fonts
}

unsafe fn list_insert_column(list: HWND, index: usize, title: &str, width: i32) {
    let mut title = wide(title);
    let mut column = LVCOLUMNW {
        mask: LVCF_TEXT | LVCF_WIDTH,
        cx: width,
        pszText: PWSTR(title.as_mut_ptr()),
        ..Default::default()
    };
    unsafe {
        SendMessageW(
            list,
            LVM_INSERTCOLUMNW,
            Some(WPARAM(index)),
            Some(LPARAM((&mut column as *mut LVCOLUMNW) as isize)),
        );
    }
}

unsafe fn list_insert_row(list: HWND, row: usize, values: [String; 5]) {
    for (column, value) in values.into_iter().enumerate() {
        let mut value = wide(&value);
        let mut item = LVITEMW {
            mask: LVIF_TEXT,
            iItem: row as i32,
            iSubItem: column as i32,
            pszText: PWSTR(value.as_mut_ptr()),
            ..Default::default()
        };
        unsafe {
            SendMessageW(
                list,
                if column == 0 {
                    LVM_INSERTITEMW
                } else {
                    LVM_SETITEMTEXTW
                },
                Some(WPARAM(row)),
                Some(LPARAM((&mut item as *mut LVITEMW) as isize)),
            );
        }
    }
}

unsafe fn combo_add(combo: HWND, text: &str) {
    let text = wide(text);
    unsafe {
        SendMessageW(
            combo,
            CB_ADDSTRING,
            None,
            Some(LPARAM(text.as_ptr() as isize)),
        );
    }
}

unsafe fn set_text(window: HWND, text: &str) {
    let text = wide(text);
    let _ = unsafe { SetWindowTextW(window, PCWSTR(text.as_ptr())) };
}

unsafe fn window_text(window: HWND) -> String {
    let length = unsafe { GetWindowTextLengthW(window) };
    let mut buffer = vec![0u16; length.max(0) as usize + 1];
    let copied = unsafe { GetWindowTextW(window, &mut buffer) };
    String::from_utf16_lossy(&buffer[..copied.max(0) as usize])
}

unsafe fn paint_preview(window: HWND, context: &DialogContext) {
    let mut paint = PAINTSTRUCT::default();
    let dc = unsafe { BeginPaint(window, &mut paint) };
    if context.sample_visible {
        unsafe {
            let _ = Rectangle(dc, 20, 475, 750, 665);
            SetBkMode(dc, TRANSPARENT);
        }
        let baseline_pen = unsafe { CreatePen(PS_SOLID, 1, COLORREF(0x0000_40E0)) };
        let glyph_pen = unsafe { CreatePen(PS_SOLID, 1, COLORREF(0x00C0_C000)) };
        let old_pen = unsafe { SelectObject(dc, HGDIOBJ(baseline_pen.0)) };
        let profile = context.model.selected_profile();
        let mut x = 35;
        let mut y = 500;
        for row in CATEGORY_ROWS {
            let adjustment = profile.adjustment_for(row.class);
            let height = (42.0 * adjustment.size_ratio.clamp(0.5, 2.0)).round() as i32;
            let family = if adjustment.font_family.is_empty() {
                "Yu Gothic UI"
            } else {
                &adjustment.font_family
            };
            let family = wide(family);
            let font = unsafe {
                CreateFontW(
                    -height,
                    0,
                    0,
                    0,
                    FW_NORMAL.0 as i32,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET,
                    OUT_DEFAULT_PRECIS,
                    CLIP_DEFAULT_PRECIS,
                    CLEARTYPE_QUALITY,
                    DEFAULT_PITCH.0 as u32,
                    PCWSTR(family.as_ptr()),
                )
            };
            let old = unsafe { SelectObject(dc, HGDIOBJ(font.0)) };
            let text: Vec<u16> = row.sample.encode_utf16().collect();
            let mut extent = SIZE::default();
            let _ = unsafe { GetTextExtentPoint32W(dc, &text, &mut extent) };
            if x + extent.cx > 730 {
                x = 35;
                y += 72;
            }
            let baseline_shift = (adjustment.baseline_shift_em * height as f64).round() as i32;
            let draw_y = y - baseline_shift;
            let mut metrics = TEXTMETRICW::default();
            let _ = unsafe { GetTextMetricsW(dc, &mut metrics) };
            let baseline_y = draw_y + metrics.tmAscent;
            unsafe {
                let _ = TextOutW(dc, x, draw_y, &text);
                draw_preview_guides(
                    dc,
                    row.sample,
                    x,
                    baseline_y,
                    extent.cx,
                    HGDIOBJ(baseline_pen.0),
                    HGDIOBJ(glyph_pen.0),
                );
                SelectObject(dc, old);
                let _ = DeleteObject(HGDIOBJ(font.0));
            }
            x += extent.cx + 22;
        }
        unsafe {
            SelectObject(dc, old_pen);
            let _ = DeleteObject(HGDIOBJ(baseline_pen.0));
            let _ = DeleteObject(HGDIOBJ(glyph_pen.0));
        }
    }
    let _ = unsafe { EndPaint(window, &paint) };
}

#[allow(clippy::too_many_arguments)]
unsafe fn draw_preview_guides(
    dc: HDC,
    text: &str,
    x: i32,
    baseline_y: i32,
    width: i32,
    baseline_pen: HGDIOBJ,
    glyph_pen: HGDIOBJ,
) {
    unsafe {
        SelectObject(dc, baseline_pen);
        draw_line(dc, x, baseline_y, x + width, baseline_y);
        SelectObject(dc, glyph_pen);
    }

    let identity = MAT2 {
        eM11: FIXED { value: 1, fract: 0 },
        eM12: FIXED::default(),
        eM21: FIXED::default(),
        eM22: FIXED { value: 1, fract: 0 },
    };
    let mut cursor_x = x;
    for character in text.chars() {
        let encoded: Vec<u16> = character.encode_utf16(&mut [0; 2]).to_vec();
        let mut character_extent = SIZE::default();
        let _ = unsafe { GetTextExtentPoint32W(dc, &encoded, &mut character_extent) };

        if character as u32 <= u16::MAX as u32 {
            let mut glyph = GLYPHMETRICS::default();
            let result = unsafe {
                GetGlyphOutlineW(
                    dc,
                    character as u32,
                    GGO_METRICS,
                    &mut glyph,
                    0,
                    None,
                    &identity,
                )
            };
            if result != GDI_ERROR as u32 && glyph.gmBlackBoxX > 0 && glyph.gmBlackBoxY > 0 {
                let left = cursor_x + glyph.gmptGlyphOrigin.x;
                let top = baseline_y - glyph.gmptGlyphOrigin.y;
                let right = left + glyph.gmBlackBoxX as i32;
                let bottom = top + glyph.gmBlackBoxY as i32;
                unsafe { draw_box(dc, left, top, right, bottom) };
            }
        }
        cursor_x += character_extent.cx;
    }
}

unsafe fn draw_box(dc: HDC, left: i32, top: i32, right: i32, bottom: i32) {
    unsafe {
        draw_line(dc, left, top, right, top);
        draw_line(dc, right, top, right, bottom);
        draw_line(dc, right, bottom, left, bottom);
        draw_line(dc, left, bottom, left, top);
    }
}

unsafe fn draw_line(dc: HDC, from_x: i32, from_y: i32, to_x: i32, to_y: i32) {
    unsafe {
        let _ = MoveToEx(dc, from_x, from_y, None);
        let _ = LineTo(dc, to_x, to_y);
    }
}

unsafe fn show_error(owner: Option<HWND>, error: &str) {
    let error = wide(error);
    unsafe {
        MessageBoxW(
            owner,
            PCWSTR(error.as_ptr()),
            w!("合成フォント"),
            MB_OK | MB_ICONERROR,
        );
    }
}

fn percent(value: f64) -> String {
    format!("{}%", plain_percent(value))
}

fn plain_percent(value: f64) -> String {
    let value = value * 100.0;
    if (value - value.round()).abs() < 0.000_001 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
