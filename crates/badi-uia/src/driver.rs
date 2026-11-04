// The UI Automation commands over COM, on the worker thread (lib.rs). Port of src/uia/BadiUia.cs with
// the same commands and the same text and JSON, so the Brain's prompts and tools work unchanged:
//   list_windows                         visible top-level windows
//   get_window_state { window, depth }   the window's element tree as text, with refs (e1, e2, ...)
//   click { ref }                        Invoke / Toggle / Select / Expand; else the point to click
//   set_value { ref, text }              ValuePattern; else focuses it so the caller types
//   focus_window { window }              bring a window to the front
//   element_at { x, y }                  what is under a screen point (for what the user marked)
// `window` is a window handle (number), or part of a title or process name; omitted = foreground.
// Refs stay valid until the next get_window_state. Coordinates are physical screen pixels.

use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};
use windows::Win32::Foundation::{CloseHandle, HWND, POINT, RECT};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW};
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, IsIconic, SW_RESTORE, SetForegroundWindow, ShowWindow};
use windows::core::{BSTR, PWSTR};

const MAX_NODES: usize = 400;
const MAX_TEXT: usize = 80;

pub struct Driver {
    uia: IUIAutomation,
    walker: IUIAutomationTreeWalker,
    refs: HashMap<String, IUIAutomationElement>,
}

fn err(e: windows::core::Error) -> String {
    e.message()
}

fn control_type(id: i32) -> &'static str {
    const NAMES: [&str; 41] = [
        "Button", "Calendar", "CheckBox", "ComboBox", "Edit", "Hyperlink", "Image", "ListItem", "List", "Menu", "MenuBar", "MenuItem", "ProgressBar", "RadioButton", "ScrollBar",
        "Slider", "Spinner", "StatusBar", "Tab", "TabItem", "Text", "ToolBar", "ToolTip", "Tree", "TreeItem", "Custom", "Group", "Thumb", "DataGrid", "DataItem", "Document",
        "SplitButton", "Window", "Pane", "Header", "HeaderItem", "Table", "TitleBar", "Separator", "SemanticZoom", "AppBar",
    ];
    usize::try_from(id - 50000).ok().and_then(|i| NAMES.get(i)).copied().unwrap_or("Custom")
}

fn trim(s: &str) -> String {
    let s = s.replace(['\r', '\n'], " ").replace('"', "'");
    let s = s.trim();
    if s.chars().count() > MAX_TEXT { format!("{}…", s.chars().take(MAX_TEXT).collect::<String>()) } else { s.to_string() }
}

fn process_name(pid: i32) -> String {
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid as u32) else { return String::new() };
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        if !ok {
            return String::new();
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        let file = path.rsplit(['\\', '/']).next().unwrap_or("");
        file.strip_suffix(".exe").or_else(|| file.strip_suffix(".EXE")).unwrap_or(file).to_string()
    }
}

fn rect(r: RECT) -> Option<(i32, i32, i32, i32)> {
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    (w > 0 && h > 0).then_some((r.left, r.top, w, h))
}

fn arg_str(a: &Value, k: &str) -> Option<String> {
    match &a[k] {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

impl Driver {
    pub fn new() -> Result<Driver, String> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let uia: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).map_err(err)?;
            let walker = uia.ControlViewWalker().map_err(err)?;
            Ok(Driver { uia, walker, refs: HashMap::new() })
        }
    }

    pub fn run(&mut self, cmd: &str, a: &Value) -> Result<Value, String> {
        match cmd {
            "list_windows" => self.list_windows(),
            "get_window_state" => {
                let w = self.find_window(arg_str(a, "window").as_deref())?;
                Ok(Value::String(self.window_state(&w, a["depth"].as_i64().unwrap_or(25))))
            }
            "click" => {
                let el = self.by_ref(arg_str(a, "ref").as_deref())?;
                click(&el)
            }
            "set_value" => {
                let el = self.by_ref(arg_str(a, "ref").as_deref())?;
                set_value(&el, &arg_str(a, "text").unwrap_or_default())
            }
            "focus_window" => {
                let w = self.find_window(arg_str(a, "window").as_deref())?;
                focus(&w)
            }
            "element_at" => {
                let (x, y) = (a["x"].as_f64().ok_or("x is required")?, a["y"].as_f64().ok_or("y is required")?);
                self.element_at(x, y)
            }
            _ => Err(format!("Unknown command {cmd}")),
        }
    }

    fn children(&self, el: &IUIAutomationElement) -> Vec<IUIAutomationElement> {
        let mut out = Vec::new();
        let mut child = unsafe { self.walker.GetFirstChildElement(el) }.ok();
        while let Some(c) = child {
            child = unsafe { self.walker.GetNextSiblingElement(&c) }.ok();
            out.push(c);
            if out.len() > 2000 {
                break;
            }
        }
        out
    }

    // ---------- windows ----------
    fn top_windows(&self) -> Vec<IUIAutomationElement> {
        let Ok(root) = (unsafe { self.uia.GetRootElement() }) else { return vec![] };
        self.children(&root)
            .into_iter()
            .filter(|w| unsafe {
                w.CurrentControlType().map(|t| t.0) == Ok(UIA_WindowControlTypeId.0)
                    && !w.CurrentIsOffscreen().map(|b| b.as_bool()).unwrap_or(true)
                    && w.CurrentName().map(|n| !n.to_string().trim().is_empty()).unwrap_or(false)
            })
            .collect()
    }

    fn list_windows(&self) -> Result<Value, String> {
        let fg = unsafe { GetForegroundWindow() }.0 as isize;
        let list: Vec<Value> = self
            .top_windows()
            .iter()
            .map(|w| unsafe {
                let handle = w.CurrentNativeWindowHandle().map(|h| h.0 as isize).unwrap_or(0);
                let b = w.CurrentBoundingRectangle().ok().and_then(rect);
                json!({
                    "window": handle,
                    "title": w.CurrentName().map(|n| n.to_string()).unwrap_or_default(),
                    "process": process_name(w.CurrentProcessId().unwrap_or(0)),
                    "focused": handle == fg,
                    "bounds": b.map(|(x, y, w, h)| json!({ "x": x, "y": y, "width": w, "height": h })),
                })
            })
            .collect();
        Ok(Value::Array(list))
    }

    fn find_window(&self, q: Option<&str>) -> Result<IUIAutomationElement, String> {
        let q = q.map(str::trim).filter(|q| !q.is_empty());
        let Some(q) = q else {
            let fg = unsafe { GetForegroundWindow() };
            if fg.0.is_null() {
                return Err("No window is in front. Pass a window from list_windows.".into());
            }
            return unsafe { self.uia.ElementFromHandle(fg) }.map_err(err);
        };
        if let Ok(handle) = q.parse::<i64>() {
            return unsafe { self.uia.ElementFromHandle(HWND(handle as isize as *mut _)) }.map_err(err);
        }
        let lower = q.to_lowercase();
        self.top_windows()
            .into_iter()
            .find(|w| unsafe {
                let name = w.CurrentName().map(|n| n.to_string().to_lowercase()).unwrap_or_default();
                name.contains(&lower) || process_name(w.CurrentProcessId().unwrap_or(0)).to_lowercase().contains(&lower)
            })
            .ok_or_else(|| format!("No open window matches \"{q}\". Use list_windows."))
    }

    // ---------- the tree ----------
    fn window_state(&mut self, w: &IUIAutomationElement, max_depth: i64) -> String {
        self.refs.clear();
        let mut out = String::new();
        unsafe {
            let name = w.CurrentName().map(|n| n.to_string()).unwrap_or_default();
            let process = process_name(w.CurrentProcessId().unwrap_or(0));
            let handle = w.CurrentNativeWindowHandle().map(|h| h.0 as isize).unwrap_or(0);
            out.push_str(&format!("Window \"{name}\" ({process}, window {handle})\n"));
        }
        let mut count = 0;
        self.walk(w, 0, 0, max_depth.clamp(1, 40) as usize, &mut out, &mut count);
        if count >= MAX_NODES {
            out.push_str(&format!("… (tree cut at {MAX_NODES} elements; use a narrower window or zoom in with a screenshot)\n"));
        }
        if count == 0 {
            out.push_str("(no accessible elements: this app draws its own UI; use the computer tools)\n");
        }
        out
    }

    // depth: shown nesting (indentation); level: real nesting, capped so deep unnamed containers can't run away.
    fn walk(&mut self, el: &IUIAutomationElement, depth: usize, level: usize, max_depth: usize, out: &mut String, count: &mut usize) {
        if depth >= max_depth || level >= 60 || *count >= MAX_NODES {
            return;
        }
        for child in self.children(el) {
            if *count >= MAX_NODES {
                return;
            }
            let mut shown = false;
            if !unsafe { child.CurrentIsOffscreen() }.map(|b| b.as_bool()).unwrap_or(true)
                && let Some(line) = describe(&child) {
                    *count += 1;
                    let r = format!("e{count}");
                    out.push_str(&" ".repeat(depth * 2));
                    out.push_str(&format!("[{r}] {line}\n"));
                    self.refs.insert(r, child.clone());
                    shown = true;
                }
            // Unnamed containers are skipped but their children still show, one level up.
            self.walk(&child, if shown { depth + 1 } else { depth }, level + 1, max_depth, out, count);
        }
    }

    fn by_ref(&self, r: Option<&str>) -> Result<IUIAutomationElement, String> {
        let r = r.unwrap_or("");
        self.refs.get(r).cloned().ok_or_else(|| format!("Unknown ref \"{r}\". Call get_window_state again for fresh refs."))
    }

    // ---------- what the user marked ----------
    // The element under a point, in one line: its type, name and value (never a password), the nearest
    // named element around it when it has no name itself, and its window.
    fn element_at(&self, x: f64, y: f64) -> Result<Value, String> {
        let el = unsafe { self.uia.ElementFromPoint(POINT { x: x as i32, y: y as i32 }) }.map_err(err)?;
        unsafe {
            let kind = control_type(el.CurrentControlType().map(|t| t.0).unwrap_or(0));
            let name = trim(&el.CurrentName().map(|n| n.to_string()).unwrap_or_default());
            let password = el.CurrentIsPassword().map(|b| b.as_bool()).unwrap_or(false);
            let value = if password { None } else { el.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId).ok().and_then(|p| p.CurrentValue().ok()).map(|v| trim(&v.to_string())) };
            let root = self.uia.GetRootElement().ok();
            let (mut around, mut window) = (None, None);
            let mut parent = el.clone();
            for _ in 0..30 {
                let Ok(p) = self.walker.GetParentElement(&parent) else { break };
                if root.as_ref().is_some_and(|r| self.uia.CompareElements(r, &p).map(|b| b.as_bool()).unwrap_or(false)) {
                    break;
                }
                let pname = trim(&p.CurrentName().map(|n| n.to_string()).unwrap_or_default());
                let ptype = p.CurrentControlType().map(|t| t.0).unwrap_or(0);
                if around.is_none() && name.is_empty() && !pname.is_empty() {
                    around = Some(format!("{} \"{pname}\"", control_type(ptype)));
                }
                if ptype == UIA_WindowControlTypeId.0 {
                    window = Some(pname);
                }
                parent = p;
            }
            let mut s = kind.to_string();
            if !name.is_empty() {
                s.push_str(&format!(" \"{name}\""));
            }
            if let Some(v) = value.filter(|v| !v.is_empty() && *v != name) {
                s.push_str(&format!(" value=\"{v}\""));
            }
            if let Some(a) = around {
                s.push_str(&format!(" inside {a}"));
            }
            if let Some(w) = window.filter(|w| !w.is_empty()) {
                s.push_str(&format!(" in window \"{w}\""));
            }
            Ok(Value::String(s))
        }
    }
}

// Nameless layout containers add nothing.
fn describe(el: &IUIAutomationElement) -> Option<String> {
    static CONTAINERS: std::sync::LazyLock<HashSet<&str>> = std::sync::LazyLock::new(|| ["pane", "group", "custom", "document", "window"].into());
    unsafe {
        let kind = control_type(el.CurrentControlType().map(|t| t.0).unwrap_or(0));
        let name = trim(&el.CurrentName().map(|n| n.to_string()).unwrap_or_default());
        let mut actions: Vec<String> = Vec::new();
        if el.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId).is_ok() {
            actions.push("click".into());
        }
        if let Ok(p) = el.GetCurrentPatternAs::<IUIAutomationTogglePattern>(UIA_TogglePatternId) {
            let state = match p.CurrentToggleState() {
                Ok(s) if s == ToggleState_On => "on",
                Ok(s) if s == ToggleState_Indeterminate => "indeterminate",
                _ => "off",
            };
            actions.push(format!("toggle:{state}"));
        }
        if let Ok(p) = el.GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(UIA_SelectionItemPatternId) {
            actions.push(if p.CurrentIsSelected().map(|b| b.as_bool()).unwrap_or(false) { "selected".into() } else { "select".into() });
        }
        if let Ok(p) = el.GetCurrentPatternAs::<IUIAutomationExpandCollapsePattern>(UIA_ExpandCollapsePatternId) {
            let state = match p.CurrentExpandCollapseState() {
                Ok(s) if s == ExpandCollapseState_Expanded => "expanded",
                Ok(s) if s == ExpandCollapseState_PartiallyExpanded => "partiallyexpanded",
                Ok(s) if s == ExpandCollapseState_LeafNode => "leafnode",
                _ => "collapsed",
            };
            actions.push(state.into());
        }
        let mut value = None;
        if let Ok(p) = el.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) {
            if el.CurrentIsPassword().map(|b| b.as_bool()).unwrap_or(false) {
                actions.push("password".into());
            } else {
                value = Some(trim(&p.CurrentValue().map(|v| v.to_string()).unwrap_or_default()));
            }
            if !p.CurrentIsReadOnly().map(|b| b.as_bool()).unwrap_or(true) {
                actions.push("editable".into());
            }
        }
        if name.is_empty() && value.is_none() && actions.is_empty() && (CONTAINERS.contains(kind.to_lowercase().as_str()) || kind == "Text") {
            return None;
        }
        let mut s = kind.to_string();
        if !name.is_empty() {
            s.push_str(&format!(" \"{name}\""));
        }
        if let Some(v) = value.filter(|v| !v.is_empty() && *v != name) {
            s.push_str(&format!(" value=\"{v}\""));
        }
        if !actions.is_empty() {
            s.push_str(&format!(" ({})", actions.join(", ")));
        }
        if !el.CurrentIsEnabled().map(|b| b.as_bool()).unwrap_or(true) {
            s.push_str(" disabled");
        }
        if el.CurrentHasKeyboardFocus().map(|b| b.as_bool()).unwrap_or(false) {
            s.push_str(" focused");
        }
        if let Some((x, y, w, h)) = el.CurrentBoundingRectangle().ok().and_then(rect) {
            s.push_str(&format!(" @{x},{y} {w}x{h}"));
        }
        Some(s)
    }
}

fn center(el: &IUIAutomationElement) -> Value {
    match unsafe { el.CurrentBoundingRectangle() }.ok().and_then(rect) {
        Some((x, y, w, h)) => json!({ "x": x + w / 2, "y": y + h / 2 }),
        None => Value::Null,
    }
}

fn focus(w: &IUIAutomationElement) -> Result<Value, String> {
    unsafe {
        let h = HWND(w.CurrentNativeWindowHandle().map(|h| h.0).unwrap_or(std::ptr::null_mut()));
        if IsIconic(h).as_bool() {
            let _ = ShowWindow(h, SW_RESTORE);
        }
        let _ = w.SetFocus();
        let _ = SetForegroundWindow(h);
        Ok(Value::String(format!("Focused \"{}\".", w.CurrentName().map(|n| n.to_string()).unwrap_or_default())))
    }
}

fn click(el: &IUIAutomationElement) -> Result<Value, String> {
    unsafe {
        let name = el.CurrentName().map(|n| n.to_string()).unwrap_or_default();
        if !el.CurrentIsEnabled().map(|b| b.as_bool()).unwrap_or(true) {
            return Err(format!("\"{name}\" is disabled."));
        }
        let at = center(el);
        let how = if let Ok(p) = el.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId) {
            p.Invoke().map_err(err)?;
            Some("invoked")
        } else if let Ok(p) = el.GetCurrentPatternAs::<IUIAutomationTogglePattern>(UIA_TogglePatternId) {
            p.Toggle().map_err(err)?;
            Some("toggled")
        } else if let Ok(p) = el.GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(UIA_SelectionItemPatternId) {
            p.Select().map_err(err)?;
            Some("selected")
        } else if let Ok(p) = el.GetCurrentPatternAs::<IUIAutomationExpandCollapsePattern>(UIA_ExpandCollapsePatternId) {
            if p.CurrentExpandCollapseState() == Ok(ExpandCollapseState_Expanded) {
                p.Collapse().map_err(err)?;
                Some("collapsed")
            } else {
                p.Expand().map_err(err)?;
                Some("expanded")
            }
        } else {
            None // No pattern to drive it: the caller clicks the center with the mouse.
        };
        Ok(json!({ "done": how.is_some(), "how": how, "name": name, "at": at }))
    }
}

fn set_value(el: &IUIAutomationElement, text: &str) -> Result<Value, String> {
    unsafe {
        let name = el.CurrentName().map(|n| n.to_string()).unwrap_or_default();
        if el.CurrentIsPassword().map(|b| b.as_bool()).unwrap_or(false) {
            return Err("This is a password field. Never type passwords: ask the user to fill it in themselves.".into());
        }
        if !el.CurrentIsEnabled().map(|b| b.as_bool()).unwrap_or(true) {
            return Err(format!("\"{name}\" is disabled."));
        }
        if let Ok(p) = el.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
            && !p.CurrentIsReadOnly().map(|b| b.as_bool()).unwrap_or(true) {
                p.SetValue(&BSTR::from(text)).map_err(err)?;
                return Ok(json!({ "done": true, "name": name, "at": center(el) }));
            }
        // Not settable directly: focus it so the caller can type the text.
        let _ = el.SetFocus();
        Ok(json!({ "done": false, "name": name, "at": center(el) }))
    }
}
