// UI Automation against real apps (port of test/uia.calculator.js). Windows only; they open Calculator
// and a small WinForms window for a few seconds, so they are ignored by default:
//   cargo test -p badi-uia --test apps -- --ignored --test-threads=1
#![cfg(windows)]

use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant};

use badi_uia::Uia;
use regex::Regex;
use serde_json::{Value, json};

fn find_window(uia: &Uia, matches: impl Fn(&str) -> bool) -> Option<Value> {
    for _ in 0..40 {
        sleep(Duration::from_millis(250));
        let list = uia.request("list_windows", json!({}), None).ok()?;
        if let Some(w) = list.as_array()?.iter().find(|w| matches(w["title"].as_str().unwrap_or(""))) {
            return Some(w.clone());
        }
    }
    None
}

fn ref_of(tree: &str, re: &str) -> String {
    let re = Regex::new(re).unwrap();
    let line = tree.lines().find(|l| re.is_match(l)).unwrap_or_else(|| panic!("element {re} in the tree:\n{tree}"));
    Regex::new(r"\[(e\d+)\]").unwrap().captures(line).unwrap()[1].to_string()
}

#[test]
#[ignore]
fn drives_calculator_through_its_accessibility_tree() {
    let uia = Uia::new();
    Command::new("cmd").args(["/C", "start", "calc.exe"]).status().unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let calc_re = Regex::new(r"(?i)calculator|kalkulator").unwrap();
        let calc = find_window(&uia, |t| calc_re.is_match(t)).expect("Calculator window found");
        let window = calc["window"].to_string();
        // Calculator is a UWP app: its frame window shows up before the app inside it has loaded.
        let one = Regex::new(r#"Button "(One|Satu)""#).unwrap();
        let mut tree = String::new();
        for _ in 0..20 {
            tree = uia.request("get_window_state", json!({ "window": window }), None).unwrap().as_str().unwrap().to_string();
            if one.is_match(&tree) {
                break;
            }
            sleep(Duration::from_millis(250));
        }
        for re in [r#"Button "(One|Satu)""#, r#"Button "(Plus|Tambah)""#, r#"Button "(Two|Dua)""#, r#"Button "(Equals|Sama dengan)""#] {
            let r = uia.request("click", json!({ "ref": ref_of(&tree, re) }), None).unwrap();
            assert_eq!(r["done"], true, "{re}: {r}");
        }
        sleep(Duration::from_millis(300));
        tree = uia.request("get_window_state", json!({ "window": window }), None).unwrap().as_str().unwrap().to_string();
        assert!(Regex::new("Display is 3|Tampilan adalah 3").unwrap().is_match(&tree), "{tree}");
        assert!(uia.request("click", json!({ "ref": "e99999" }), None).unwrap_err().contains("Unknown ref"));
        // What is under a point (used for what the user marks): the Equals button, by its position in the tree.
        let eq = tree.lines().find(|l| Regex::new(r#"Button "(Equals|Sama dengan)""#).unwrap().is_match(l)).unwrap();
        let m = Regex::new(r"@(-?\d+),(-?\d+) (\d+)x(\d+)").unwrap().captures(eq).unwrap();
        let n = |i: usize| m[i].parse::<f64>().unwrap();
        let started = Instant::now();
        let under = uia.request("element_at", json!({ "x": n(1) + n(3) / 2.0, "y": n(2) + n(4) / 2.0 }), None).unwrap();
        println!("element_at took {} ms: {under}", started.elapsed().as_millis());
        assert!(Regex::new(r#"Button "(Equals|Sama dengan)".*in window "(Calculator|Kalkulator)""#).unwrap().is_match(under.as_str().unwrap()), "{under}");
    }));
    uia.stop();
    let _ = Command::new("taskkill").args(["/IM", "CalculatorApp.exe", "/F"]).output();
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}

#[test]
#[ignore]
fn fills_a_text_field_and_refuses_a_password_field() {
    let uia = Uia::new();
    // A small WinForms window with a text box and a password box.
    let script = [
        "Add-Type -AssemblyName System.Windows.Forms",
        "$f = New-Object Windows.Forms.Form; $f.Text = 'aina UIA Test'; $f.Width = 360; $f.Height = 180",
        "$t = New-Object Windows.Forms.TextBox; $t.Name = 'nama'; $t.AccessibleName = 'Nama'; $t.Top = 20; $t.Left = 20; $t.Width = 300",
        "$p = New-Object Windows.Forms.TextBox; $p.AccessibleName = 'Sandi'; $p.UseSystemPasswordChar = $true; $p.Top = 60; $p.Left = 20; $p.Width = 300",
        "$f.Controls.Add($t); $f.Controls.Add($p); [void]$f.ShowDialog()",
    ]
    .join("; ");
    let mut ps = Command::new("powershell.exe").args(["-NoProfile", "-Command", &script]).spawn().unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let win = find_window(&uia, |t| t == "aina UIA Test").expect("test window found");
        let window = win["window"].to_string();
        let tree = uia.request("get_window_state", json!({ "window": window }), None).unwrap().as_str().unwrap().to_string();
        let r = uia.request("set_value", json!({ "ref": ref_of(&tree, r#"Edit "Nama""#), "text": "halo badi" }), None).unwrap();
        assert_eq!(r["done"], true);
        let e = uia.request("set_value", json!({ "ref": ref_of(&tree, r#"Edit "Sandi""#), "text": "secret" }), None).unwrap_err();
        assert!(e.to_lowercase().contains("password"), "{e}");
        let tree = uia.request("get_window_state", json!({ "window": window }), None).unwrap().as_str().unwrap().to_string();
        assert!(tree.contains(r#"Edit "Nama" value="halo badi""#), "{tree}");
        assert!(Regex::new(r#"Edit "Sandi" \([^)]*password"#).unwrap().is_match(&tree), "{tree}"); // a password field, never its value
    }));
    uia.stop();
    let _ = ps.kill();
    let _ = ps.wait();
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}
