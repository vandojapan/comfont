use std::{
    ffi::c_void,
    mem::size_of,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
};

use aviutl2::generic::EditHandle;
use compositefont_core::ProfileDocument;
use windows::{
    Win32::{
        Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM},
        Graphics::Gdi::{
            BeginPaint, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, COLOR_WINDOW, CreateFontW,
            CreatePen, DEFAULT_CHARSET, DEFAULT_GUI_FONT, DEFAULT_PITCH, DeleteObject, EndPaint,
            FIXED, FW_NORMAL, GDI_ERROR, GGO_METRICS, GLYPHMETRICS, GM_ADVANCED, GetGlyphOutlineW,
            GetStockObject, GetSysColorBrush, GetTextExtentPoint32W, GetTextMetricsW, HDC, HGDIOBJ,
            InvalidateRect, LineTo, MAT2, MoveToEx, OUT_DEFAULT_PRECIS, PAINTSTRUCT, PS_SOLID,
            SelectObject, SetBkMode, SetGraphicsMode, SetWorldTransform, TEXTMETRICW, TRANSPARENT,
            TextOutW, XFORM,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Controls::{
                BST_CHECKED, CB_SETMINVISIBLE, COMBOBOXINFO, EM_SETSEL, GetComboBoxInfo,
                ICC_LISTVIEW_CLASSES, INITCOMMONCONTROLSEX, InitCommonControlsEx,
                LIST_VIEW_ITEM_STATE_FLAGS, LVCF_TEXT, LVCF_WIDTH, LVCOLUMNW, LVIF_TEXT,
                LVIS_FOCUSED, LVIS_SELECTED, LVITEMW, LVM_DELETEALLITEMS, LVM_GETITEMCOUNT,
                LVM_GETNEXTITEM, LVM_INSERTCOLUMNW, LVM_INSERTITEMW, LVM_SETEXTENDEDLISTVIEWSTYLE,
                LVM_SETITEMSTATE, LVM_SETITEMTEXTW, LVN_ITEMCHANGED, LVNI_SELECTED,
                LVS_EX_DOUBLEBUFFER, LVS_EX_FULLROWSELECT, LVS_EX_GRIDLINES, LVS_REPORT,
                LVS_SHOWSELALWAYS, NMLISTVIEW, ShowScrollBar, WC_LISTVIEWW,
            },
            Input::KeyboardAndMouse::EnableWindow,
            WindowsAndMessaging::{
                BM_GETCHECK, BN_CLICKED, BS_AUTOCHECKBOX, BS_DEFPUSHBUTTON, BS_GROUPBOX,
                CB_ADDSTRING, CB_GETCOUNT, CB_GETCURSEL, CB_GETDROPPEDSTATE, CB_RESETCONTENT,
                CB_SETCURSEL, CBN_DROPDOWN, CBN_SELCHANGE, CBS_AUTOHSCROLL, CBS_DROPDOWN,
                CBS_DROPDOWNLIST, CBS_NOINTEGRALHEIGHT, CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW,
                CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
                ES_AUTOHSCROLL, GWL_STYLE, GetMessageW, GetWindowLongPtrW, GetWindowRect,
                GetWindowTextLengthW, GetWindowTextW, HMENU, IDC_ARROW, IsDialogMessageW, IsWindow,
                LB_GETTOPINDEX, LB_SETTOPINDEX, LoadCursorW, MB_ICONERROR, MB_ICONINFORMATION,
                MB_OK, MSG, MessageBoxW, RegisterClassW, SB_VERT, SW_HIDE, SW_SHOW,
                SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
                SendMessageW, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, SetWindowTextW,
                ShowWindow, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_COMMAND,
                WM_CREATE, WM_CTLCOLORBTN, WM_CTLCOLORSTATIC, WM_MOUSEWHEEL, WM_NCCREATE,
                WM_NOTIFY, WM_PAINT, WM_SETFONT, WNDCLASSW, WS_BORDER, WS_CAPTION, WS_CHILD,
                WS_CLIPCHILDREN, WS_EX_CLIENTEDGE, WS_EX_CONTROLPARENT, WS_EX_DLGMODALFRAME,
                WS_GROUP, WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
            },
        },
    },
    core::{PCWSTR, PWSTR, w},
};

use crate::{
    font_collection::{self, FontRegistrationState, FontStageOutcome},
    model::{CATEGORY_ROWS, EditorModel},
    storage::save_document,
};

const CLASS_NAME: PCWSTR = w!("CompositeFontEditorWindow");
const PREVIEW_CLASS_NAME: PCWSTR = w!("CompositeFontPreviewWindow");
const WINDOW_WIDTH: i32 = 800;
const WINDOW_HEIGHT: i32 = 790;
const PREVIEW_WIDTH: i32 = 730;
const PREVIEW_HEIGHT: i32 = 190;

const ID_PROFILE: usize = 100;
const ID_UNIT: usize = 101;
const ID_LIST: usize = 110;
const ID_SELECTED_CATEGORY: usize = 111;
const ID_FONT: usize = 120;
const ID_SIZE: usize = 121;
const ID_BASELINE: usize = 122;
const ID_TRACKING: usize = 123;
const ID_APPLY: usize = 124;
const ID_VERTICAL_SCALE: usize = 125;
const ID_HORIZONTAL_SCALE: usize = 126;
const ID_ROW_ADD: usize = 127;
const ID_ROW_REMOVE: usize = 128;
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
    vertical_scale: HWND,
    horizontal_scale: HWND,
    row_add: HWND,
    row_remove: HWND,
    sample_visible: HWND,
    preview: HWND,
}

struct DialogContext {
    model: EditorModel,
    persisted_document: ProfileDocument,
    profile_path: PathBuf,
    edit_handle: Arc<EditHandle>,
    font_registration: Arc<Mutex<FontRegistrationState>>,
    font_names: Vec<String>,
    controls: Controls,
    refreshing: bool,
    sample_visible: bool,
}

impl DialogContext {
    fn new(
        document: ProfileDocument,
        profile_path: PathBuf,
        edit_handle: Arc<EditHandle>,
        font_registration: Arc<Mutex<FontRegistrationState>>,
    ) -> Self {
        let font_names = query_font_names(&edit_handle);
        Self {
            model: EditorModel::new(document.clone()),
            persisted_document: document,
            profile_path,
            edit_handle,
            font_registration,
            font_names,
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
    edit_handle: Arc<EditHandle>,
    font_registration: Arc<Mutex<FontRegistrationState>>,
) -> Result<ProfileDocument, String> {
    register_window_class()?;
    let owner = HWND(owner.hwnd.get() as *mut c_void);
    let mut context = Box::new(DialogContext::new(
        document,
        profile_path,
        edit_handle,
        font_registration,
    ));
    let context_ptr = (&mut *context) as *mut DialogContext;
    let instance = module_instance()?;

    let window = unsafe {
        CreateWindowExW(
            WS_EX_DLGMODALFRAME | WS_EX_CONTROLPARENT,
            CLASS_NAME,
            w!("合成フォント設定"),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_CLIPCHILDREN,
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
            if message.message == WM_MOUSEWHEEL
                && scroll_font_dropdown(context.controls.font, message.wParam)
            {
                continue;
            }
            if message.message == WM_MOUSEWHEEL
                && adjust_numeric_field(&mut context, message.wParam, message.lParam)
            {
                continue;
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
                hbrBackground: GetSysColorBrush(COLOR_WINDOW),
                lpszClassName: CLASS_NAME,
                ..Default::default()
            };
            if RegisterClassW(&class) == 0 {
                return Err(format!(
                    "合成フォント画面のウィンドウクラスを登録できません: {}",
                    std::io::Error::last_os_error()
                ));
            }
            let preview_class = WNDCLASSW {
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(preview_window_proc),
                hInstance: instance,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                hbrBackground: GetSysColorBrush(COLOR_WINDOW),
                lpszClassName: PREVIEW_CLASS_NAME,
                ..Default::default()
            };
            if RegisterClassW(&preview_class) == 0 {
                return Err(format!(
                    "プレビュー用ウィンドウクラスを登録できません: {}",
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
                let focused_row_changed = notification.hdr.idFrom == ID_LIST
                    && notification.hdr.code == LVN_ITEMCHANGED
                    && notification.iItem >= 0
                    && notification.uNewState & LVIS_FOCUSED.0 != 0
                    && !context.refreshing;
                if focused_row_changed
                    && select_table_row(&mut context.model, notification.iItem as usize)
                {
                    unsafe {
                        refresh_editor_fields(context);
                        refresh_row_remove_enabled(context);
                    }
                }
            }
            LRESULT(0)
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => {
            let dc = HDC(wparam.0 as *mut c_void);
            unsafe { SetBkMode(dc, TRANSPARENT) };
            LRESULT(unsafe { GetSysColorBrush(COLOR_WINDOW) }.0 as isize)
        }
        WM_CLOSE => {
            let _ = unsafe { DestroyWindow(window) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

unsafe extern "system" fn preview_window_proc(
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
    let context = unsafe {
        GetWindowLongPtrW(
            window,
            windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
        ) as *const DialogContext
    };
    if message == WM_PAINT && !context.is_null() {
        unsafe { paint_preview(window, &*context) };
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
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
        create_label(window, instance, "プロファイル：", 20, 20, 95, 24, 0, font)?;
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
                | WINDOW_STYLE(LVS_REPORT | LVS_SHOWSELALWAYS),
            WS_EX_CLIENTEDGE,
            20,
            55,
            730,
            190,
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
            ("文字種", 90),
            ("フォント", 210),
            ("サイズ", 70),
            ("ベース", 70),
            ("字送り", 70),
            ("垂直比率", 85),
            ("水平比率", 85),
        ]
        .into_iter()
        .enumerate()
        {
            list_insert_column(context.controls.list, index, title, width);
        }

        context.controls.row_add = create_child(
            window,
            instance,
            w!("BUTTON"),
            "+",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            WINDOW_EX_STYLE(0),
            20,
            250,
            32,
            26,
            ID_ROW_ADD,
            font,
        )?;
        context.controls.row_remove = create_child(
            window,
            instance,
            w!("BUTTON"),
            "−",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            WINDOW_EX_STYLE(0),
            58,
            250,
            32,
            26,
            ID_ROW_REMOVE,
            font,
        )?;
        create_label(
            window,
            instance,
            "同じ文字種は上から優先",
            105,
            254,
            180,
            20,
            0,
            font,
        )?;

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
            55,
            24,
            ID_SELECTED_CATEGORY,
            font,
        )?;
        create_label(window, instance, "フォント", 100, 297, 70, 20, 0, font)?;
        context.controls.font = create_child(
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
            95,
            318,
            190,
            220,
            ID_FONT,
            font,
        )?;
        create_label(window, instance, "サイズ", 295, 297, 60, 20, 0, font)?;
        context.controls.size = create_edit(window, instance, 290, 318, 60, ID_SIZE, font)?;
        create_label(window, instance, "ベース", 360, 297, 60, 20, 0, font)?;
        context.controls.baseline = create_edit(window, instance, 355, 318, 60, ID_BASELINE, font)?;
        create_label(window, instance, "字送り", 425, 297, 60, 20, 0, font)?;
        context.controls.tracking = create_edit(window, instance, 420, 318, 60, ID_TRACKING, font)?;
        create_label(window, instance, "垂直比率", 490, 297, 65, 20, 0, font)?;
        context.controls.vertical_scale =
            create_edit(window, instance, 485, 318, 65, ID_VERTICAL_SCALE, font)?;
        create_label(window, instance, "水平比率", 560, 297, 65, 20, 0, font)?;
        context.controls.horizontal_scale =
            create_edit(window, instance, 555, 318, 65, ID_HORIZONTAL_SCALE, font)?;
        create_child(
            window,
            instance,
            w!("BUTTON"),
            "適用",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            WINDOW_EX_STYLE(0),
            630,
            317,
            105,
            26,
            ID_APPLY,
            font,
        )?;
        for (text, x, width, id) in [
            ("新規…", 20, 100, ID_NEW),
            ("保存", 130, 100, ID_SAVE),
            ("プロファイル削除", 240, 140, ID_DELETE),
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

        context.controls.preview = create_preview(window, instance, context)?;

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
        configure_font_dropdown(context.controls.font);
        refresh_all(context);
    }
    Ok(())
}

unsafe fn create_preview(
    parent: HWND,
    instance: HINSTANCE,
    context: &mut DialogContext,
) -> Result<HWND, String> {
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PREVIEW_CLASS_NAME,
            w!(""),
            WS_CHILD | WS_VISIBLE | WS_BORDER,
            20,
            475,
            PREVIEW_WIDTH,
            PREVIEW_HEIGHT,
            Some(parent),
            None,
            Some(instance),
            Some((context as *mut DialogContext).cast()),
        )
        .map_err(|error| format!("プレビュー領域を作成できません: {error}"))
    }
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
        (ID_FONT, CBN_DROPDOWN) => unsafe {
            rebuild_font_combo(context);
            configure_font_dropdown(context.controls.font);
        },
        (ID_ROW_ADD, BN_CLICKED) => match unsafe { apply_editor_fields(context) } {
            Ok(()) => unsafe {
                context.model.add_selected_adjustment();
                context.refreshing = true;
                refresh_list(context);
                refresh_editor_fields(context);
                context.refreshing = false;
                refresh_preview(context);
            },
            Err(error) => unsafe { show_error(Some(window), &error) },
        },
        (ID_ROW_REMOVE, BN_CLICKED) => unsafe {
            let rows = selected_table_rows(context.controls.list);
            if context.model.remove_adjustments_for_table_rows(&rows) > 0 {
                context.refreshing = true;
                refresh_list(context);
                refresh_editor_fields(context);
                context.refreshing = false;
                refresh_preview(context);
            }
        },
        (ID_PROFILE, CBN_SELCHANGE) if !context.refreshing => {
            if let Err(error) = unsafe { apply_editor_fields(context) } {
                unsafe { show_error(Some(window), &error) };
                return;
            }
            let index = unsafe {
                SendMessageW(context.controls.profile, CB_GETCURSEL, None, None).0 as usize
            };
            if context.model.select_profile(index) {
                unsafe { refresh_all(context) };
            }
        }
        (ID_APPLY, BN_CLICKED) => match unsafe { apply_editor_fields(context) } {
            Ok(()) => unsafe {
                refresh_selected_list_rows(context);
                refresh_preview(context);
            },
            Err(error) => unsafe { show_error(Some(window), &error) },
        },
        (ID_NEW, BN_CLICKED) => {
            if let Err(error) = unsafe { apply_editor_fields(context) } {
                unsafe { show_error(Some(window), &error) };
                return;
            }
            context.model.create_profile();
            unsafe { refresh_all(context) };
        }
        (ID_DELETE, BN_CLICKED) => match context.model.delete_selected_profile() {
            Ok(()) => unsafe { refresh_all(context) },
            Err(error) => unsafe { show_error(Some(window), &error) },
        },
        (ID_SAVE, BN_CLICKED) => match unsafe {
            commit_and_save(context).and_then(|()| scan_private_fonts_after_save(context))
        } {
            Ok(outcome) => unsafe {
                let message = save_confirmation_message(&outcome);
                let message = wide(&message);
                MessageBoxW(
                    Some(window),
                    PCWSTR(message.as_ptr()),
                    w!("プロファイルの保存"),
                    MB_OK | MB_ICONINFORMATION,
                );
                refresh_all(context);
            },
            Err(error) => unsafe { show_error(Some(window), &error) },
        },
        (ID_SAMPLE_VISIBLE, BN_CLICKED) => {
            context.sample_visible = unsafe {
                SendMessageW(context.controls.sample_visible, BM_GETCHECK, None, None).0 as u32
                    == BST_CHECKED.0
            };
            unsafe {
                if context.sample_visible {
                    let _ = ShowWindow(context.controls.preview, SW_SHOW);
                    refresh_preview(context);
                } else {
                    let _ = ShowWindow(context.controls.preview, SW_HIDE);
                }
            }
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

fn save_confirmation_message(outcome: &FontStageOutcome) -> String {
    let font_status = if outcome.source_file_count == 0 {
        String::new()
    } else if outcome.added_file_count == 0 {
        format!(
            "\n追加フォント{}件は次回起動用フォルダーに配置済みです。",
            outcome.existing_file_count
        )
    } else {
        format!(
            "\n追加フォント{}件を次回起動用フォルダーに配置しました。使用するにはAviUtl2を再起動してください。",
            outcome.added_file_count
        )
    };

    format!(
        "プロファイルを保存しました。{font_status}\n\nこのビルドはFontManagerへ合成プロファイルを追加登録しません。\nプロファイル名は標準のフォント一覧には表示されません。\n「合成フォント字幕」またはcompositefont.decorate(...)から使用してください。"
    )
}

unsafe fn commit_and_save(context: &mut DialogContext) -> Result<(), String> {
    unsafe { apply_editor_fields(context)? };
    let document = context.model.document().clone();
    save_document(&context.profile_path, &document)?;
    context.persisted_document = document;
    Ok(())
}

fn scan_private_fonts_after_save(context: &mut DialogContext) -> Result<FontStageOutcome, String> {
    let outcome = context
        .font_registration
        .lock()
        .map_err(|_| "profile was saved, but font registration state is poisoned".to_owned())?
        .stage_fonts_for_next_launch()
        .map_err(|error| format!("profile was saved, but private font staging failed: {error}"))?;

    if outcome.source_file_count == 0 {
        let _ = aviutl2::logger::write_info_log(&format!(
            "Composite Font: Save button found no private font files in {}",
            font_collection::font_directory().display()
        ));
    } else {
        let _ = aviutl2::logger::write_info_log(&format!(
            "Composite Font: Save button copied {} new private font file(s) to {}; {} file(s) already existed; restart is required",
            outcome.added_file_count,
            font_collection::host_font_directory().display(),
            outcome.existing_file_count
        ));
    }
    Ok(outcome)
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
    let vertical_scale = parse_number(
        &unsafe { window_text(context.controls.vertical_scale) },
        "垂直比率",
    )?;
    let horizontal_scale = parse_number(
        &unsafe { window_text(context.controls.horizontal_scale) },
        "水平比率",
    )?;
    let mut rows = unsafe { selected_table_rows(context.controls.list) };
    if rows.is_empty() {
        rows.push(context.model.selected_table_row());
    }
    context.model.update_table_rows(
        &rows,
        font,
        size,
        baseline,
        tracking,
        vertical_scale,
        horizontal_scale,
    )
}

fn parse_number(text: &str, label: &str) -> Result<f64, String> {
    let text = text.trim().trim_end_matches('%').trim();
    text.parse::<f64>()
        .map_err(|_| format!("{label}に数値を入力してください。"))
}

unsafe fn refresh_all(context: &mut DialogContext) {
    context.refreshing = true;
    unsafe {
        refresh_profile_combo(context);
        refresh_list(context);
        refresh_editor_fields(context);
    }
    context.refreshing = false;
    unsafe { refresh_preview(context) };
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
    let mut row_index = 0;
    for row in CATEGORY_ROWS {
        unsafe {
            insert_adjustment_row(
                context.controls.list,
                row_index,
                row.label,
                profile.adjustment_for(row.class),
            );
        }
        row_index += 1;
        for adjustment in profile.fallbacks_for(row.class) {
            unsafe {
                insert_adjustment_row(context.controls.list, row_index, row.label, adjustment);
            }
            row_index += 1;
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
            Some(WPARAM(context.model.selected_table_row())),
            Some(LPARAM((&mut item as *mut LVITEMW) as isize)),
        );
    }
}

unsafe fn refresh_selected_list_rows(context: &DialogContext) {
    let mut rows = unsafe { selected_table_rows(context.controls.list) };
    if rows.is_empty() {
        rows.push(context.model.selected_table_row());
    }
    for row in rows {
        let Some(adjustment) = context.model.adjustment_for_table_row(row) else {
            continue;
        };
        let font = if adjustment.font_family.is_empty() {
            "（変更なし）"
        } else {
            &adjustment.font_family
        };
        for (column, value) in [
            font.to_owned(),
            percent(adjustment.size_ratio),
            percent(adjustment.baseline_shift_em),
            percent(adjustment.tracking_adjust_em),
            percent(adjustment.vertical_scale_ratio),
            percent(adjustment.horizontal_scale_ratio),
        ]
        .into_iter()
        .enumerate()
        {
            unsafe { list_set_cell(context.controls.list, row, column + 1, &value) };
        }
    }
}

unsafe fn refresh_editor_fields(context: &DialogContext) {
    let row = CATEGORY_ROWS[context.model.selected_category_index()];
    let adjustment = context.model.selected_adjustment();
    unsafe {
        set_text(context.controls.selected_category, row.label);
        set_text(context.controls.font, &adjustment.font_family);
        refresh_row_remove_enabled(context);
        set_text(context.controls.size, &plain_percent(adjustment.size_ratio));
        set_text(
            context.controls.baseline,
            &plain_percent(adjustment.baseline_shift_em),
        );
        set_text(
            context.controls.tracking,
            &plain_percent(adjustment.tracking_adjust_em),
        );
        set_text(
            context.controls.vertical_scale,
            &plain_percent(adjustment.vertical_scale_ratio),
        );
        set_text(
            context.controls.horizontal_scale,
            &plain_percent(adjustment.horizontal_scale_ratio),
        );
    }
}

unsafe fn insert_adjustment_row(
    list: HWND,
    row: usize,
    label: &str,
    adjustment: &compositefont_core::FontAdjustment,
) {
    let font = if adjustment.font_family.is_empty() {
        "（変更なし）".to_owned()
    } else {
        adjustment.font_family.clone()
    };
    unsafe {
        list_insert_row(
            list,
            row,
            [
                label.to_owned(),
                font,
                percent(adjustment.size_ratio),
                percent(adjustment.baseline_shift_em),
                percent(adjustment.tracking_adjust_em),
                percent(adjustment.vertical_scale_ratio),
                percent(adjustment.horizontal_scale_ratio),
            ],
        );
    }
}

unsafe fn selected_table_rows(list: HWND) -> Vec<usize> {
    let mut rows = Vec::new();
    let mut current = -1_isize;
    let item_count = unsafe { SendMessageW(list, LVM_GETITEMCOUNT, None, None).0.max(0) as usize };
    for _ in 0..item_count {
        let next = unsafe {
            SendMessageW(
                list,
                LVM_GETNEXTITEM,
                Some(WPARAM(current as usize)),
                Some(LPARAM(LVNI_SELECTED as isize)),
            )
            .0
        };
        if next < 0 || next <= current {
            break;
        }
        rows.push(next as usize);
        current = next;
    }
    rows
}

unsafe fn refresh_row_remove_enabled(context: &DialogContext) {
    let enabled = unsafe { selected_table_rows(context.controls.list) }
        .into_iter()
        .any(|row| context.model.is_additional_table_row(row));
    let _ = unsafe { EnableWindow(context.controls.row_remove, enabled) };
}

fn select_table_row(model: &mut EditorModel, row: usize) -> bool {
    model.select_table_row(row)
}

unsafe fn refresh_preview(context: &DialogContext) {
    if context.sample_visible && !context.controls.preview.is_invalid() {
        let _ = unsafe { InvalidateRect(Some(context.controls.preview), None, true) };
    }
}

unsafe fn fill_font_combo(context: &DialogContext) {
    let mut fonts = context.font_names.clone();
    for profile in &context.model.document().profiles {
        for row in CATEGORY_ROWS {
            let family = &profile.adjustment_for(row.class).font_family;
            if !family.is_empty() {
                fonts.push(family.clone());
            }
        }
        for row in CATEGORY_ROWS {
            fonts.extend(
                profile
                    .fallbacks_for(row.class)
                    .iter()
                    .map(|adjustment| adjustment.font_family.clone()),
            );
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

fn query_font_names(edit_handle: &EditHandle) -> Vec<String> {
    normalize_font_names(edit_handle.get_font_names())
}

fn normalize_font_names(mut names: Vec<String>) -> Vec<String> {
    names.retain(|name| !name.is_empty());
    names.sort_unstable();
    names.dedup();
    names
}

unsafe fn rebuild_font_combo(context: &mut DialogContext) {
    let current_font = unsafe { window_text(context.controls.font) };
    context.font_names = query_font_names(&context.edit_handle);

    context.refreshing = true;
    unsafe {
        SendMessageW(context.controls.font, CB_RESETCONTENT, None, None);
        fill_font_combo(context);
        set_text(context.controls.font, &current_font);
    }
    context.refreshing = false;
}

unsafe fn configure_font_dropdown(combo: HWND) {
    let mut info = COMBOBOXINFO {
        cbSize: size_of::<COMBOBOXINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetComboBoxInfo(combo, &mut info) }.is_err() || info.hwndList.is_invalid() {
        return;
    }

    let style = unsafe { GetWindowLongPtrW(info.hwndList, GWL_STYLE) };
    unsafe {
        SetWindowLongPtrW(info.hwndList, GWL_STYLE, style | WS_VSCROLL.0 as isize);
        let _ = SetWindowPos(
            info.hwndList,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
        let _ = ShowScrollBar(info.hwndList, SB_VERT, true);
    }
}

unsafe fn scroll_font_dropdown(combo: HWND, wheel: WPARAM) -> bool {
    if combo.is_invalid() || unsafe { SendMessageW(combo, CB_GETDROPPEDSTATE, None, None) }.0 == 0 {
        return false;
    }

    let mut info = COMBOBOXINFO {
        cbSize: size_of::<COMBOBOXINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetComboBoxInfo(combo, &mut info) }.is_err() || info.hwndList.is_invalid() {
        return false;
    }

    let delta = ((wheel.0 >> 16) as u16) as i16 as i32;
    let current = unsafe { SendMessageW(info.hwndList, LB_GETTOPINDEX, None, None) }.0 as i32;
    let count = unsafe { SendMessageW(combo, CB_GETCOUNT, None, None) }.0 as i32;
    let target = wheel_target_top(current, count, delta);
    unsafe {
        SendMessageW(
            info.hwndList,
            LB_SETTOPINDEX,
            Some(WPARAM(target as usize)),
            None,
        );
        let _ = ShowScrollBar(info.hwndList, SB_VERT, true);
    }
    true
}

unsafe fn adjust_numeric_field(context: &mut DialogContext, wheel: WPARAM, cursor: LPARAM) -> bool {
    let point = POINT {
        x: (cursor.0 as u16) as i16 as i32,
        y: ((cursor.0 >> 16) as u16) as i16 as i32,
    };
    let (control, minimum) = if unsafe { point_in_window(point, context.controls.size) } {
        (context.controls.size, Some(1.0))
    } else if unsafe { point_in_window(point, context.controls.baseline) } {
        (context.controls.baseline, None)
    } else if unsafe { point_in_window(point, context.controls.tracking) } {
        (context.controls.tracking, None)
    } else if unsafe { point_in_window(point, context.controls.vertical_scale) } {
        (context.controls.vertical_scale, Some(1.0))
    } else if unsafe { point_in_window(point, context.controls.horizontal_scale) } {
        (context.controls.horizontal_scale, Some(1.0))
    } else {
        return false;
    };

    let original = unsafe { window_text(control) };
    let Ok(current) = parse_number(&original, "") else {
        return false;
    };
    let delta = ((wheel.0 >> 16) as u16) as i16 as i32;
    let Some(value) = numeric_wheel_value(current, delta, minimum) else {
        return true;
    };
    let text = plain_number(value);
    unsafe {
        set_text(control, &text);
        SendMessageW(
            control,
            EM_SETSEL,
            Some(WPARAM(text.encode_utf16().count())),
            Some(LPARAM(text.encode_utf16().count() as isize)),
        );
    }

    if unsafe { apply_editor_fields(context) }.is_err() {
        unsafe { set_text(control, &original) };
        return true;
    }

    unsafe {
        refresh_selected_list_rows(context);
        refresh_preview(context);
    }
    true
}

unsafe fn point_in_window(point: POINT, window: HWND) -> bool {
    let mut bounds = RECT::default();
    unsafe { GetWindowRect(window, &mut bounds) }.is_ok()
        && point.x >= bounds.left
        && point.x < bounds.right
        && point.y >= bounds.top
        && point.y < bounds.bottom
}

fn numeric_wheel_value(current: f64, delta: i32, minimum: Option<f64>) -> Option<f64> {
    if delta == 0 || !current.is_finite() {
        return None;
    }
    let notches = delta.unsigned_abs().div_ceil(120) as f64;
    let direction = if delta > 0 { 1.0 } else { -1.0 };
    let value = current + direction * notches;
    Some(minimum.map_or(value, |minimum| value.max(minimum)))
}

fn wheel_target_top(current: i32, count: i32, delta: i32) -> i32 {
    if count <= 0 || delta == 0 {
        return current.max(0);
    }
    let notches = delta.unsigned_abs().div_ceil(120) as i32;
    let direction = if delta > 0 { -1 } else { 1 };
    (current + direction * notches * 3).clamp(0, count - 1)
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

unsafe fn list_insert_row(list: HWND, row: usize, values: [String; 7]) {
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

unsafe fn list_set_cell(list: HWND, row: usize, column: usize, value: &str) {
    let mut value = wide(value);
    let mut item = LVITEMW {
        iSubItem: column as i32,
        pszText: PWSTR(value.as_mut_ptr()),
        ..Default::default()
    };
    unsafe {
        SendMessageW(
            list,
            LVM_SETITEMTEXTW,
            Some(WPARAM(row)),
            Some(LPARAM((&mut item as *mut LVITEMW) as isize)),
        );
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
            SetBkMode(dc, TRANSPARENT);
            SetGraphicsMode(dc, GM_ADVANCED);
        }
        let baseline_pen = unsafe { CreatePen(PS_SOLID, 1, COLORREF(0x0000_40E0)) };
        let glyph_pen = unsafe { CreatePen(PS_SOLID, 1, COLORREF(0x00C0_C000)) };
        let old_pen = unsafe { SelectObject(dc, HGDIOBJ(baseline_pen.0)) };
        let profile = context.model.selected_profile();
        let mut x = 15;
        let mut y = 25;
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
            let tracking = (adjustment.tracking_adjust_em * height as f64).round() as i32;
            let run_width = unsafe { measure_preview_run(dc, row.sample, tracking) };
            let horizontal_scale = adjustment.horizontal_scale_ratio.clamp(0.01, 10.0);
            let vertical_scale = adjustment.vertical_scale_ratio.clamp(0.01, 10.0);
            let rendered_width = (run_width as f64 * horizontal_scale).round() as i32;
            if x + rendered_width > PREVIEW_WIDTH - 20 {
                x = 15;
                y += 72;
            }
            let baseline_shift = (adjustment.baseline_shift_em * height as f64).round() as i32;
            let draw_y = y - baseline_shift;
            let mut metrics = TEXTMETRICW::default();
            let _ = unsafe { GetTextMetricsW(dc, &mut metrics) };
            let baseline_y = draw_y + metrics.tmAscent;
            let transform = scale_about(x, draw_y, horizontal_scale as f32, vertical_scale as f32);
            unsafe {
                let _ = SetWorldTransform(dc, &transform);
                draw_preview_run(
                    dc,
                    row.sample,
                    x,
                    draw_y,
                    baseline_y,
                    run_width,
                    tracking,
                    HGDIOBJ(baseline_pen.0),
                    HGDIOBJ(glyph_pen.0),
                );
                let _ = SetWorldTransform(
                    dc,
                    &XFORM {
                        eM11: 1.0,
                        eM22: 1.0,
                        ..Default::default()
                    },
                );
                SelectObject(dc, old);
                let _ = DeleteObject(HGDIOBJ(font.0));
            }
            x += rendered_width + 22;
        }
        unsafe {
            SelectObject(dc, old_pen);
            let _ = DeleteObject(HGDIOBJ(baseline_pen.0));
            let _ = DeleteObject(HGDIOBJ(glyph_pen.0));
        }
    }
    let _ = unsafe { EndPaint(window, &paint) };
}

fn scale_about(x: i32, y: i32, horizontal: f32, vertical: f32) -> XFORM {
    XFORM {
        eM11: horizontal,
        eM22: vertical,
        eDx: x as f32 * (1.0 - horizontal),
        eDy: y as f32 * (1.0 - vertical),
        ..Default::default()
    }
}

unsafe fn measure_preview_run(dc: HDC, text: &str, tracking: i32) -> i32 {
    let mut advances = Vec::with_capacity(text.chars().count());
    for character in text.chars() {
        let mut buffer = [0; 2];
        let encoded = character.encode_utf16(&mut buffer);
        let mut extent = SIZE::default();
        let _ = unsafe { GetTextExtentPoint32W(dc, encoded, &mut extent) };
        advances.push(extent.cx);
    }
    tracked_run_width(&advances, tracking)
}

fn tracked_run_width(advances: &[i32], tracking: i32) -> i32 {
    advances.iter().sum::<i32>() + tracking * advances.len().saturating_sub(1) as i32
}

#[allow(clippy::too_many_arguments)]
unsafe fn draw_preview_run(
    dc: HDC,
    text: &str,
    x: i32,
    draw_y: i32,
    baseline_y: i32,
    width: i32,
    tracking: i32,
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
    let character_count = text.chars().count();
    for (index, character) in text.chars().enumerate() {
        let mut buffer = [0; 2];
        let encoded = character.encode_utf16(&mut buffer);
        let mut character_extent = SIZE::default();
        let _ = unsafe { GetTextExtentPoint32W(dc, encoded, &mut character_extent) };
        let _ = unsafe { TextOutW(dc, cursor_x, draw_y, encoded) };

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
        if index + 1 < character_count {
            cursor_x += tracking;
        }
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
    plain_number(value * 100.0)
}

fn plain_number(value: f64) -> String {
    if (value - value.round()).abs() < 0.000_001 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_font_names, numeric_wheel_value, save_confirmation_message, scale_about,
        select_table_row, tracked_run_width, wheel_target_top,
    };
    use crate::{font_collection::FontStageOutcome, model::EditorModel};
    use compositefont_core::ProfileDocument;

    #[test]
    fn maps_repeated_character_class_rows_before_the_next_category() {
        let mut model = EditorModel::new(ProfileDocument::with_builtin_default());
        model.add_selected_adjustment();
        model.add_selected_adjustment();
        assert_eq!(model.selected_table_row(), 2);

        assert!(select_table_row(&mut model, 1));
        assert_eq!(model.selected_fallback_index(), Some(0));
        assert!(select_table_row(&mut model, 3));
        assert_eq!(model.selected_category_index(), 1);
        assert_eq!(model.selected_fallback_index(), None);

        model.add_selected_adjustment();
        assert_eq!(model.selected_category_index(), 1);
        assert_eq!(model.selected_fallback_index(), Some(0));
        assert_eq!(model.selected_table_row(), 4);
    }

    #[test]
    fn font_names_are_sorted_deduplicated_and_empty_names_are_removed() {
        assert_eq!(
            normalize_font_names(vec![
                "Yu Gothic UI".to_owned(),
                "FreeSans".to_owned(),
                String::new(),
                "FreeSans".to_owned(),
            ]),
            vec!["FreeSans".to_owned(), "Yu Gothic UI".to_owned()]
        );
    }

    #[test]
    fn save_message_explains_that_profiles_are_not_registered_as_fonts() {
        let message = save_confirmation_message(&FontStageOutcome::default());

        assert!(message.contains("FontManagerへ合成プロファイルを追加登録しません"));
        assert!(message.contains("標準のフォント一覧には表示されません"));
        assert!(message.contains("compositefont.decorate(...)"));
        assert!(!message.contains("監視先"));
    }

    #[test]
    fn tracking_changes_only_gaps_between_glyphs() {
        assert_eq!(tracked_run_width(&[10, 12, 8], 3), 36);
        assert_eq!(tracked_run_width(&[10, 12, 8], -2), 26);
        assert_eq!(tracked_run_width(&[10], 20), 10);
    }

    #[test]
    fn scale_transform_keeps_the_preview_anchor_fixed() {
        let transform = scale_about(20, 30, 1.25, 0.8);
        assert_eq!(transform.eM11, 1.25);
        assert_eq!(transform.eM22, 0.8);
        assert_eq!(20.0 * transform.eM11 + transform.eDx, 20.0);
        assert_eq!(30.0 * transform.eM22 + transform.eDy, 30.0);
    }

    #[test]
    fn wheel_adjusts_numeric_values_one_point_per_notch() {
        assert_eq!(numeric_wheel_value(100.0, 120, Some(1.0)), Some(101.0));
        assert_eq!(numeric_wheel_value(100.0, -120, Some(1.0)), Some(99.0));
        assert_eq!(numeric_wheel_value(0.0, 240, None), Some(2.0));
    }

    #[test]
    fn size_wheel_does_not_reach_zero() {
        assert_eq!(numeric_wheel_value(1.0, -120, Some(1.0)), Some(1.0));
        assert_eq!(numeric_wheel_value(2.0, -240, Some(1.0)), Some(1.0));
    }

    #[test]
    fn wheel_moves_three_rows_per_notch() {
        assert_eq!(wheel_target_top(10, 100, -120), 13);
        assert_eq!(wheel_target_top(10, 100, 120), 7);
        assert_eq!(wheel_target_top(10, 100, -240), 16);
    }

    #[test]
    fn wheel_stays_inside_font_list() {
        assert_eq!(wheel_target_top(1, 100, 120), 0);
        assert_eq!(wheel_target_top(98, 100, -120), 99);
        assert_eq!(wheel_target_top(0, 0, -120), 0);
    }
}
