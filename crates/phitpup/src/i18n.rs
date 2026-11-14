//! Thai or English for the settings window, the tray and the messages:
//! the `language` setting, which by default follows the Windows display
//! language. `PHITPUP_LANG=th` or `en` overrides both (for previews and
//! testing, and to tell `phitpup-update.exe` which language to use).
//!
//! The English text is the key: [`t`] returns the Thai for it, or the
//! English if there is none. Placeholders like `{key}` are filled in by the
//! caller with `str::replace`, so both languages can put them anywhere.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

use phitpup_core::config::Language;
use windows_sys::Win32::Globalization::GetUserDefaultUILanguage;

/// The `language` setting: 0 auto, 1 Thai, 2 English.
static CHOSEN: AtomicU8 = AtomicU8::new(0);

/// Follows the `language` setting from now on.
pub fn set(language: Language) {
    let n = match language {
        Language::Auto => 0,
        Language::Thai => 1,
        Language::English => 2,
    };
    CHOSEN.store(n, Ordering::Relaxed);
}

/// Whether to speak Thai.
pub fn thai() -> bool {
    static FORCED: OnceLock<Option<bool>> = OnceLock::new();
    let forced = FORCED.get_or_init(|| match std::env::var("PHITPUP_LANG").as_deref() {
        Ok("th") => Some(true),
        Ok("en") => Some(false),
        _ => None,
    });
    match (forced, CHOSEN.load(Ordering::Relaxed)) {
        (Some(th), _) => *th,
        (None, 1) => true,
        (None, 2) => false,
        _ => windows_thai(),
    }
}

/// Whether the Windows display language is Thai.
pub fn windows_thai() -> bool {
    static THAI: OnceLock<bool> = OnceLock::new();
    // SAFETY: plain query. LANG_THAI is primary language 0x1E.
    *THAI.get_or_init(|| unsafe { GetUserDefaultUILanguage() } & 0x3FF == 0x1E)
}

/// `en` in the user's language.
pub fn t(en: &'static str) -> &'static str {
    if !thai() {
        return en;
    }
    thai_for(en).unwrap_or(en)
}

/// [`t`] for text made at run time, such as what the updater reports.
pub fn t_dyn(en: &str) -> String {
    match thai().then(|| thai_for(en)).flatten() {
        Some(th) => th.to_string(),
        None => en.to_string(),
    }
}

fn thai_for(en: &str) -> Option<&'static str> {
    Some(match en {
        // Pages
        "Getting started" => "เริ่มใช้งาน",
        "PhitPup is running. It has no window of its own: it waits by the clock at the bottom right of the screen (under ^ if you do not see its icon) and fixes text typed in the wrong keyboard layout." => {
            "ผิดปุ๊บทำงานอยู่แล้ว ไม่มีหน้าต่างของตัวเอง แต่รออยู่ข้างนาฬิกาที่มุมขวาล่างของจอ (กด ^ ถ้าไม่เห็น) คอยแก้ข้อความที่พิมพ์ผิดภาษาให้"
        }
        "While you type" => "ระหว่างพิมพ์",
        "A bubble shows the fix under the word. {key} takes it; Esc leaves the word as typed." => {
            "ป้ายจะขึ้นใต้คำ พร้อมคำที่ถูก กด {key} เพื่อรับ กด Esc ถ้าตั้งใจพิมพ์แบบนั้น"
        }
        "Just typed it" => "พิมพ์ไปแล้ว",
        "{key} fixes the last word. Press again to reach one more word back, and once more to undo." => {
            "กด {key} เพื่อแก้คำล่าสุด กดซ้ำเพื่อย้อนไปอีกคำ กดอีกครั้งเพื่อยกเลิก"
        }
        "An older word" => "คำเก่าที่ผ่านไปแล้ว",
        "Click on the word, or select the text, then press {key}." => {
            "คลิกที่คำนั้น หรือลากเลือกข้อความ แล้วกด {key}"
        }
        "Try it in Notepad" => "ลองใน Notepad",
        "Type l;ylfu there and see." => "ลองพิมพ์ l;ylfu ดู",
        "General" => "ทั่วไป",
        // The pill beside the caret after a word was fixed on its own.
        "Fixed" => "แก้แล้ว",
        "Programs" => "โปรแกรม",
        "Words" => "คำ",
        "Shortcuts" => "ปุ่มลัด",
        "About" => "เกี่ยวกับ",
        "PhitPup Settings" => "ตั้งค่าผิดปุ๊บ",
        "PhitPup · paused" => "PhitPup · หยุดชั่วคราว",
        "Version {v}" => "รุ่น {v}",

        // General
        "PhitPup is on" => "ผิดปุ๊บเปิดอยู่",
        "Watching for text typed in the wrong layout." => "คอยดูข้อความที่พิมพ์ผิดภาษาอยู่",
        "Paused. Every key passes through untouched." => "หยุดชั่วคราว ทุกปุ่มที่กดผ่านไปตามปกติ",
        "The settings window, tray menu and messages." => "หน้าตั้งค่า เมนูที่ไอคอน และข้อความต่างๆ",
        "Like Windows" => "ตาม Windows",
        "When to offer a fix" => "จะให้แก้ตอนไหน",
        "The hotkey always works." => "ปุ่มลัดใช้ได้เสมอ",
        "Suggest while typing" => "เสนอคำที่ถูกระหว่างพิมพ์",
        "A bubble shows the fix. {key} accepts it, Esc hides it." => {
            "ป้ายเล็ก ๆ บอกคำที่ถูก กด {key} เพื่อใช้ หรือ Esc เพื่อซ่อน"
        }
        "Fix automatically when I press Space" => "แก้ให้เองเมื่อกด Space",
        "Only words it is very sure about. A note shows each fix; {key} right after undoes it." => {
            "เฉพาะคำที่มั่นใจมาก มีป้ายบอกทุกครั้งที่แก้ กด {key} ทันทีเพื่อคืนคำเดิม"
        }
        "Only when I press {key}" => "เฉพาะตอนกด {key}",
        "Fixes the last word; press again to reach further back." => {
            "แก้คำล่าสุด กดอีกครั้งเพื่อย้อนไปคำก่อนหน้า"
        }
        "Underline the word instead of a bubble" => "ขีดเส้นใต้คำแทนป้าย",
        "Apps that cannot show it get the bubble." => "โปรแกรมที่ขีดเส้นใต้ไม่ได้จะยังเห็นป้าย",
        "Fix Thai typed in the wrong order" => "แก้ภาษาไทยที่พิมพ์สลับลำดับ",
        "Such as เเม่ → แม่, ทํา → ทำ, ก่ี → กี่. Tab accepts." => {
            "เช่น เเม่ → แม่, ทํา → ทำ, ก่ี → กี่ กด Tab เพื่อแก้"
        }
        "Fix Caps Lock slips and doubled words" => "แก้ตอนลืมปิด Caps Lock และคำที่พิมพ์ซ้ำ",
        "Such as hELLO \u{2192} Hello and the the \u{2192} the." => {
            "เช่น hELLO \u{2192} Hello และ the the \u{2192} the"
        }
        "Suggest words typed with a slip" => "เสนอคำที่พิมพ์พลาด",
        "Such as \u{0E2A}\u{0E49}\u{0E27}\u{0E2A}\u{0E14}\u{0E35} \u{2192} \u{0E2A}\u{0E27}\u{0E31}\u{0E2A}\u{0E14}\u{0E35} and teh \u{2192} the. Only suggested." => {
            "เช่น ส้วสดี \u{2192} สวัสดี และ teh \u{2192} the เสนอให้เท่านั้น ไม่แก้เอง"
        }
        "Show the bubble above the line" => "แสดงฟองเหนือบรรทัด",
        "Keeps search suggestions and other lists in view." => {
            "ไม่บังรายการที่เด้งใต้ช่องพิมพ์ เช่น คำค้นที่แนะนำ"
        }
        "Leave it" => "ไม่ต้องแก้",
        "Fix on its own after 5 Tabs" => "แก้เองเมื่อกด Tab คำเดิมครบ 5 ครั้ง",
        "A word whose fix you take 5 times is then fixed on Space." => {
            "คำที่คุณกดแก้ครบ 5 ครั้ง จะแก้ให้เองตอนเว้นวรรค"
        }
        "Keeps those words and how often you took their fix on this computer only. Turning it off forgets them." => {
            "เก็บคำเหล่านั้นและจำนวนครั้งที่กดแก้ไว้ในเครื่องนี้เท่านั้น ปิดแล้วจะลืมทั้งหมด"
        }
        "Learn my own words" => "จำคำของฉัน",
        "A word you type on past 5 times is no longer offered." => {
            "คำที่ถูกเสนอแก้แต่คุณพิมพ์ต่อครบ 5 ครั้ง จะไม่เสนออีก"
        }
        "Keeps only words PhitPup offered to fix, on this computer only; nothing else you type. Turning it off forgets them." => {
            "เก็บเฉพาะคำที่ผิดปุ๊บเคยเสนอแก้ ไว้ในเครื่องนี้เท่านั้น ไม่เก็บคำอื่นที่คุณพิมพ์ ปิดแล้วจะลืมทั้งหมด"
        }
        "Fixed on their own ({n})" => "แก้เอง ({n})",
        "Words whose fix you take with the accept key 5 times show up here." => {
            "คำที่คุณกดแก้ครบ 5 ครั้งจะขึ้นที่นี่"
        }
        "Your words ({n})" => "คำของคุณ ({n})",
        "Words PhitPup offered to fix that you typed on past 5 times show up here." => {
            "คำที่ถูกเสนอแก้แต่คุณพิมพ์ต่อครบ 5 ครั้งจะขึ้นที่นี่"
        }
        "Forget what it learned" => "ลืมสิ่งที่เรียนรู้",
        "Start with Windows" => "เปิดพร้อม Windows",
        "Run PhitPup when you sign in." => "เปิดผิดปุ๊บเมื่อเข้าสู่ระบบ",
        "Turn off in full-screen windows" => "ปิดในหน้าต่างเต็มจอ",
        "Games, videos and presentations." => "เกม วิดีโอ และงานนำเสนอ",
        "Settings file" => "ไฟล์ตั้งค่า",
        "Open" => "เปิด",

        // Programs
        "Choose programs where PhitPup stays out of the way." => {
            "เลือกโปรแกรมที่ไม่ต้องการให้ผิดปุ๊บเข้าไปยุ่ง"
        }
        "Your programs" => "โปรแกรมของคุณ",
        "Add a program" => "เพิ่มโปรแกรม",
        "Nothing added yet. Add a game or an app where PhitPup should do nothing." => {
            "ยังไม่มี เพิ่มเกมหรือแอปที่ไม่ต้องการให้ผิดปุ๊บทำงาน"
        }
        "Off: PhitPup does nothing here." => "ปิด: ผิดปุ๊บไม่ทำอะไรในโปรแกรมนี้",
        "No bubble: only {key} works here." => "ไม่มีป้าย: ใช้ได้แค่ {key}",
        "Off" => "ปิด",
        "No bubble" => "ไม่มีป้าย",
        "Built in" => "มีให้แล้ว",
        "No other programs are open" => "ไม่มีโปรแกรมอื่นเปิดอยู่",
        "Always off: password fields, password managers (KeePass, 1Password, Bitwarden and others), Windows sign-in prompts, remote desktop and virtual machines." => {
            "ปิดเสมอ: ช่องรหัสผ่าน โปรแกรมจัดการรหัสผ่าน (KeePass, 1Password, Bitwarden และอื่น ๆ) หน้าต่างเข้าสู่ระบบของ Windows, Remote Desktop และเครื่องเสมือน"
        }
        "No bubble: terminals and code editors (Windows Terminal, PowerShell, VS Code, Cursor, Visual Studio, JetBrains IDEs and others)." => {
            "ไม่มีป้าย: เทอร์มินัลและโปรแกรมเขียนโค้ด (Windows Terminal, PowerShell, VS Code, Cursor, Visual Studio, JetBrains และอื่น ๆ)"
        }

        // Words
        "Words PhitPup should leave alone, and words it should always fix. Add them as you type them." => {
            "คำที่ไม่ต้องให้ผิดปุ๊บแก้ และคำที่ให้แก้เสมอ เพิ่มได้ตอนพิมพ์"
        }
        "Learn from my corrections" => "เรียนรู้จากที่ฉันแก้",
        "Remembers fixes you undo, keep, or hide with Esc 3 times." => {
            "จำคำที่คุณคืนกลับหรือเก็บไว้ และคำที่กด Esc ซ่อนครบ 3 ครั้ง"
        }
        "A word to leave alone, e.g. gdp" => "คำที่ไม่ต้องแก้ เช่น gdp",
        "Add" => "เพิ่ม",
        "Leave alone ({n})" => "ไม่ต้องแก้ ({n})",
        "Press Esc three times, or Shift+Esc once, while the bubble shows a word, or type it above." => {
            "กด Esc 3 ครั้ง หรือ Shift+Esc ครั้งเดียว ตอนที่ป้ายขึ้น หรือพิมพ์ในช่องด้านบน"
        }
        "Always fix ({n})" => "แก้ให้เสมอ ({n})",
        "Words you convert with {key} and then keep typing after show up here." => {
            "คำที่คุณแก้ด้วย {key} แล้วพิมพ์ต่อจะมาอยู่ตรงนี้"
        }
        "Click a word to remove it." => "คลิกที่คำเพื่อลบออก",
        "Type a word first." => "พิมพ์คำก่อน",
        "One word at a time, without spaces." => "ทีละคำ ไม่มีช่องว่าง",
        "Quotes and backslashes can't be saved." => "บันทึกเครื่องหมายคำพูดและแบ็กสแลชไม่ได้",

        // Shortcuts
        "Fix the last word" => "แก้คำล่าสุด",
        "Press again to go one more word back." => "กดซ้ำเพื่อย้อนไปอีกคำ",
        "Accept the suggestion" => "ใช้คำที่เสนอ",
        "While the bubble is showing." => "ตอนที่ป้ายขึ้นอยู่",
        "Press the new shortcut, or Esc to cancel." => "กดปุ่มลัดใหม่ หรือ Esc เพื่อยกเลิก",
        "Press a shortcut\u{2026}" => "กดปุ่มลัด\u{2026}",
        "Also fix when I tap Shift twice" => "แก้เมื่อแตะ Shift สองครั้งด้วย",
        "Two quick taps, nothing in between." => "แตะเร็ว ๆ สองครั้ง ไม่กดปุ่มอื่นคั่น",
        "Hide the suggestion" => "ซ่อนคำที่เสนอ",
        "Until you start the next word." => "จนกว่าจะเริ่มคำถัดไป",
        "Never suggest this word" => "ไม่ต้องเสนอคำนี้อีก",
        "Adds it to Words." => "เพิ่มลงในหน้าคำ",
        "Reset to defaults" => "คืนค่าเริ่มต้น",
        "{key} is already used for the other shortcut." => "{key} ใช้กับปุ่มลัดอีกอันอยู่แล้ว",
        "That key types a character. Add Ctrl or Alt." => "ปุ่มนี้พิมพ์ตัวอักษร ให้กด Ctrl หรือ Alt ด้วย",
        "Esc and Shift+Esc already dismiss the suggestion." => "Esc และ Shift+Esc ใช้ปิดคำที่เสนออยู่แล้ว",
        "\u{201C}{word}\u{201D} is already in the list." => "\u{201C}{word}\u{201D} อยู่ในรายการแล้ว",
        "Shortcuts with the Windows key belong to Windows." => "ปุ่มลัดที่มีปุ่ม Windows เป็นของ Windows",
        "That key can't be used. Try a letter, F-key, Space, Tab or Backspace." => {
            "ใช้ปุ่มนี้ไม่ได้ ลองตัวอักษร ปุ่ม F, Space, Tab หรือ Backspace"
        }

        // About
        "Fixes text typed in the wrong keyboard layout, Thai and English." => {
            "แก้ข้อความที่พิมพ์ผิดภาษา ไทย ⇄ อังกฤษ"
        }
        "Made by" => "ผู้พัฒนา",
        "Team" => "ทีม",
        "Version" => "รุ่น",
        "License" => "สัญญาอนุญาต",
        "Free, open source (GPL-3.0)" => "ใช้ฟรี โอเพนซอร์ส (GPL-3.0)",
        "Words fixed for you" => "คำที่แก้ให้แล้ว",
        "Updates" => "อัปเดต",
        "Remind me to check" => "เตือนให้เช็กอัปเดต",
        "Once this version is two months old." => "เมื่อรุ่นนี้อายุครบสองเดือน",
        "PhitPup works offline: it has no networking code, and the Microsoft Store does the updating. What you type is kept in memory only until you click or switch windows, and is never sent anywhere. Saved are only your settings file, including the words you add, and small notes beside it: how many words were fixed (never which), the last version that ran, when it last crashed and whether it asked for a review." => {
            "ผิดปุ๊บทำงานในเครื่อง ไม่มีโค้ดต่ออินเทอร์เน็ตเลย การอัปเดตเป็นหน้าที่ของ Microsoft Store สิ่งที่คุณพิมพ์อยู่ในหน่วยความจำจนกว่าจะคลิกหรือสลับหน้าต่างเท่านั้น และไม่ถูกส่งไปไหน สิ่งที่บันทึกมีแค่ไฟล์ตั้งค่า (รวมคำที่คุณเพิ่ม) กับบันทึกเล็ก ๆ ข้างกัน: จำนวนคำที่แก้ (ไม่เก็บว่าคำอะไร) รุ่นที่เปิดล่าสุด เวลาที่ปิดไปเองครั้งล่าสุด และเคยขอรีวิวแล้วหรือยัง"
        }
        "PhitPup works offline: the part that sees your keys has no networking code. Only when you click Check for updates does the separate phitpup-update.exe ask the website for the newest version number and, if you agree, download the installer. What you type is kept in memory only until you click or switch windows, and is never sent anywhere. The only things saved are your settings file, including the words you add, and small notes beside it: how many words were fixed (never which), the last version that ran, when it last crashed and the date of the last update reminder." => {
            "ผิดปุ๊บทำงานในเครื่อง ส่วนที่เห็นปุ่มที่คุณกดไม่มีโค้ดต่ออินเทอร์เน็ตเลย มีแค่ phitpup-update.exe ที่แยกไว้ ซึ่งต่อเน็ตเฉพาะตอนคุณกดเช็กอัปเดต เพื่อถามเลขรุ่นล่าสุด และโหลดตัวติดตั้งถ้าคุณตกลง สิ่งที่คุณพิมพ์อยู่ในหน่วยความจำจนกว่าจะคลิกหรือสลับหน้าต่างเท่านั้น และไม่ถูกส่งไปไหน สิ่งที่บันทึกมีแค่ไฟล์ตั้งค่า (รวมคำที่คุณเพิ่ม) กับบันทึกเล็ก ๆ ข้างกัน: จำนวนคำที่แก้ (ไม่เก็บว่าคำอะไร) รุ่นที่เปิดล่าสุด เวลาที่ปิดไปเองครั้งล่าสุด และวันที่เตือนอัปเดตครั้งล่าสุด"
        }
        "Opens the download page in your browser." => "เปิดหน้าดาวน์โหลดในเบราว์เซอร์",
        "Check for updates" => "เช็กอัปเดต",
        "The Microsoft Store keeps PhitPup up to date." => "Microsoft Store อัปเดตผิดปุ๊บให้เอง",
        "Open the Store" => "เปิด Store",
        "Checks for a new version and installs it for you." => "หารุ่นใหม่และติดตั้งให้",
        "Checking\u{2026}" => "กำลังเช็ก\u{2026}",
        "You have the newest version." => "เป็นรุ่นล่าสุดแล้ว",
        "Check again" => "เช็กอีกครั้ง",
        "Version {v} is out ({size})." => "มีรุ่น {v} แล้ว ({size})",
        "Version {v} is out." => "มีรุ่น {v} แล้ว",
        "Installing. If PhitPup closes, open it again from Start." => {
            "กำลังติดตั้ง ถ้าผิดปุ๊บปิดไป เปิดใหม่ได้จากเมนู Start"
        }
        "Could not reach the Microsoft Store. Check your internet connection." => {
            "ติดต่อ Microsoft Store ไม่ได้ ตรวจดูการต่ออินเทอร์เน็ต"
        }
        "The Store will install it once your PC is plugged in or on Wi-Fi." => {
            "Store จะติดตั้งให้เมื่อเสียบสายชาร์จหรือต่อ Wi-Fi"
        }
        "The Microsoft Store could not install it, so its Downloads page is open." => {
            "Microsoft Store ติดตั้งไม่สำเร็จ จึงเปิดหน้าดาวน์โหลดของ Store ให้แล้ว"
        }
        "Install {v}" => "ติดตั้ง {v}",
        "Downloading\u{2026} {pct}%" => "กำลังโหลด\u{2026} {pct}%",
        "Downloading\u{2026}" => "กำลังโหลด\u{2026}",
        "Installing. PhitPup will start again by itself." => "กำลังติดตั้ง ผิดปุ๊บจะเปิดขึ้นมาใหม่เอง",
        "Installing\u{2026}" => "กำลังติดตั้ง\u{2026}",
        "Try again" => "ลองอีกครั้ง",
        "Report a problem" => "แจ้งปัญหา",
        "Copies your version and settings, never what you type, and opens GitHub." => {
            "คัดลอกรุ่นและการตั้งค่า (ไม่มีสิ่งที่คุณพิมพ์) แล้วเปิด GitHub"
        }
        "Copied. Paste it (Ctrl+V) into the report on GitHub." => {
            "คัดลอกแล้ว วาง (Ctrl+V) ในหน้าแจ้งปัญหาบน GitHub"
        }
        "Copy and report" => "คัดลอกและแจ้ง",
        // What phitpup-update.exe reports (crates/phitpup-update).
        "Could not connect. Check your internet connection." => "เชื่อมต่อไม่ได้ ตรวจดูการต่ออินเทอร์เน็ต",
        "The connection dropped. Try again." => "การเชื่อมต่อหลุด ลองอีกครั้ง",
        "The download did not match the published SHA-256." => {
            "ไฟล์ที่โหลดไม่ตรงกับ SHA-256 ที่ประกาศไว้ จึงไม่ได้ติดตั้ง"
        }
        "The download is not there yet. Try again in a few minutes." => "ไฟล์ยังไม่พร้อม ลองใหม่อีกสักครู่",
        "The download is unexpectedly large." => "ไฟล์ใหญ่ผิดปกติ จึงไม่ได้โหลด",
        "The installer is unexpectedly large." => "ตัวติดตั้งใหญ่ผิดปกติ จึงไม่ได้โหลด",
        "A different version came out meanwhile. Check again." => {
            "ระหว่างนี้มีรุ่นใหม่กว่าออกมา ลองเช็กอีกครั้ง"
        }
        "The version information looks wrong." => "ข้อมูลรุ่นดูผิดปกติ",
        "Could not save the installer." => "บันทึกตัวติดตั้งไม่ได้",
        "Could not start the installer." => "เปิดตัวติดตั้งไม่ได้",
        "Could not start the updater." => "เปิดตัวอัปเดตไม่ได้",
        "The updater stopped unexpectedly." => "ตัวอัปเดตหยุดทำงานกะทันหัน",

        // Tray
        "PhitPup is running" => "ผิดปุ๊บทำงานอยู่",
        "PhitPup was updated to {v}" => "อัปเดตผิดปุ๊บเป็นรุ่น {v} แล้ว",
        "Switch language with ~" => "สลับภาษาด้วยปุ่ม ~",
        "Bubble on here too." => "แสดงฟองที่นี่ด้วย",
        "No bubble. Turn it on if you also chat or write here." => {
            "ไม่มีฟอง เปิดได้ถ้าใช้แชตหรือเขียนข้อความที่นี่ด้วย"
        }
        "Show the language at the cursor" => "แสดงภาษาที่เคอร์เซอร์",
        "A small TH or EN for a moment when it changes." => "ป้าย TH หรือ EN เล็ก ๆ ครู่หนึ่งเมื่อเปลี่ยนภาษา",
        "The key above Tab switches Thai and English." => "ปุ่มเหนือ Tab ใช้สลับภาษาไทย-อังกฤษ",
        "Is PhitPup helping?" => "ผิดปุ๊บช่วยคุณได้ไหม",
        "It has fixed {n} words for you so far. Click here to rate it in the Microsoft Store; a few words help others find it." => {
            "ผิดปุ๊บแก้ให้แล้ว {n} คำ คลิกตรงนี้เพื่อให้คะแนนใน Microsoft Store รีวิวสั้น ๆ ช่วยให้คนอื่นเจอผิดปุ๊บ"
        }
        "Rate PhitPup" => "ให้คะแนนผิดปุ๊บ",
        "A review in the Microsoft Store helps others find it." => {
            "รีวิวใน Microsoft Store ช่วยให้คนอื่นเจอผิดปุ๊บ"
        }
        "Rate it" => "ให้คะแนน",
        "PhitPup closed unexpectedly last time" => "ครั้งก่อนผิดปุ๊บปิดไปเองโดยไม่ตั้งใจ",
        "Sorry about that. Click here to report it: the details are copied for you to paste, and nothing is sent by itself." => {
            "ขออภัยด้วย คลิกตรงนี้เพื่อแจ้งปัญหา รายละเอียดจะถูกคัดลอกไว้ให้วาง ผิดปุ๊บไม่ส่งอะไรออกไปเอง"
        }
        "Click here to see what's new." => "คลิกตรงนี้เพื่อดูว่ามีอะไรใหม่",
        "Typed in the wrong layout? Press {accept} on the suggestion, or {hotkey}.\nClick this icon for settings." => {
            "พิมพ์ผิดภาษา? กด {accept} ตอนป้ายขึ้น หรือกด {hotkey}\nคลิกไอคอนนี้เพื่อเปิดหน้าตั้งค่า"
        }
        "PhitPup: paused" => "ผิดปุ๊บ: หยุดชั่วคราว",
        "PhitPup: fixes text typed in the wrong layout" => "ผิดปุ๊บ: แก้ข้อความที่พิมพ์ผิดภาษา",
        "A newer PhitPup may be out" => "อาจมีผิดปุ๊บรุ่นใหม่แล้ว",
        "You have version {v}, from about {months} months ago. Click here to open the download page." => {
            "คุณใช้รุ่น {v} ซึ่งออกมาประมาณ {months} เดือนแล้ว คลิกตรงนี้เพื่อเปิดหน้าดาวน์โหลด"
        }
        "Some lines in the settings file could not be read, so their defaults are used:\n{problems}\n\n{file}" => {
            "อ่านบางบรรทัดในไฟล์ตั้งค่าไม่ได้ จึงใช้ค่าเริ่มต้นแทน:\n{problems}\n\n{file}"
        }
        "Settings\u{2026}" => "ตั้งค่า\u{2026}",
        "Pause" => "หยุดชั่วคราว",
        "Suggest while typing\t{accept}" => "เสนอระหว่างพิมพ์\t{accept}",
        "Fix automatically\tSpace" => "แก้ให้เอง\tSpace",
        "Fix on hotkey only\t{hotkey}" => "แก้เมื่อกดปุ่มลัดเท่านั้น\t{hotkey}",
        "Quit PhitPup" => "ออกจากผิดปุ๊บ",

        // Start-up
        "PhitPup is already running. Look for its icon in the notification area (bottom right)." => {
            "ผิดปุ๊บเปิดอยู่แล้ว ดูไอคอนที่มุมขวาล่างข้างนาฬิกา"
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every placeholder in the English must be in the Thai too.
    #[test]
    fn thai_keeps_placeholders() {
        let keys = [
            "A bubble shows the fix under the word. {key} takes it; Esc leaves the word as typed.",
            "{key} fixes the last word. Press again to reach one more word back, and once more to undo.",
            "Click on the word, or select the text, then press {key}.",
            "A bubble shows the fix. {key} accepts it, Esc hides it.",
            "Only words it is very sure about. A note shows each fix; {key} right after undoes it.",
            "Only when I press {key}",
            "No bubble: only {key} works here.",
            "Leave alone ({n})",
            "Always fix ({n})",
            "Words you convert with {key} and then keep typing after show up here.",
            "Version {v} is out ({size}).",
            "Version {v} is out.",
            "It has fixed {n} words for you so far. Click here to rate it in the Microsoft Store; a few words help others find it.",
            "Install {v}",
            "Downloading\u{2026} {pct}%",
            "Version {v}",
            "PhitPup was updated to {v}",
            "Typed in the wrong layout? Press {accept} on the suggestion, or {hotkey}.\nClick this icon for settings.",
            "You have version {v}, from about {months} months ago. Click here to open the download page.",
            "Some lines in the settings file could not be read, so their defaults are used:\n{problems}\n\n{file}",
            "Suggest while typing\t{accept}",
            "Fix on hotkey only\t{hotkey}",
        ];
        for en in keys {
            let th = thai_for(en).unwrap_or_else(|| panic!("no Thai for {en:?}"));
            for p in [
                "{key}",
                "{n}",
                "{v}",
                "{size}",
                "{pct}",
                "{accept}",
                "{hotkey}",
                "{months}",
                "{problems}",
                "{file}",
            ] {
                assert_eq!(en.contains(p), th.contains(p), "{p} in {en:?}");
            }
        }
    }
}
