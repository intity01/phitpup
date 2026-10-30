//! Words people type on purpose that PhitPup must not get in the way of:
//! Thai chat and slang, English tech words and brands, mixed capitals, and
//! words that are fine twice.
//!
//! Each word is typed one character at a time on Kedmanee and Pattachote,
//! and every suggestion the bubble would show on the way is counted, as is
//! whether Space would fix it in auto mode. The counts may only go down:
//! when a change lowers one, lower its budget here too.

use phitpup_core::{Detector, Fixer, Layout};

const THAI_CHAT: &str = "มั้ย ไหม ป่าว เปล่า คับ ครับ งับ ค่า คะ ค่ะ จ้า จ้ะ น้า นะ อ่ะ อะ เด้อ เนอะ ป้ะ ปะ มะ ม่ะ หรา เหรอ หรอ เหรอะ ชิมิ จุงเบย จึ้ง ฟิน ปัง ชิว ชิล ตึง เริ่ด เกิ้น นอยด์ เทอ เธอ ชั้น ฉัน เค้า เขา ตัวเอง แก มึง กู ไร อะไร ยังไง ไง เปน เป็น จิง จริง ม่าย ไม่ บ่ แม่ง โคตร โครต สาด จัด ๆ อ้าว เอ้า เออ อืม อื้ม อุ๊ย ว้าว เย้ แง ฮือ ฮ่า ฮ่าๆ 555 กร๊าก ถถถ ถถ งือ ฮัลโหล บาย ไลค์ แชร์ เมนต์ คอมเมนต์ อัพ อัปเดต โพสต์ สตอรี่ ไลฟ์ แคปชั่น แฮชแท็ก เพจ กรุ๊ป แอด แอดมิน ทวิต ทวีต ติ๊กต็อก ยูทูบ เฟซ เฟสบุ๊ค ไลน์ เน็ต ไวไฟ แบต ชาร์จ โหลด ดาวน์โหลด อัปโหลด เซฟ เซ็ต เช็ค เช็ก โอเค โอเคร ด่วน ฟรี เทพ โปร มือโปร มือใหม่ เกรียน ติ่ง โอปป้า ชิปเปอร์ เมน แฟนด้อม สตรีม เวิ่น เอ๋อ งง เพลีย เหนื่อย หิว ง่วง ปวดหัว";
const EN_WORDS: &str = "linux ubuntu debian nginx apache python javascript typescript golang rust kotlin swift java ruby php perl bash zsh powershell kubernetes docker podman terraform ansible jenkins gitlab bitbucket jira confluence slack zoom teams discord twitch reddit instagram facebook twitter whatsapp telegram shopee lazada grab foodpanda lineman agoda booking airbnb uber spotify youtube netflix tiktok canva figma notion trello asana miro chatgpt openai anthropic claude gemini copilot midjourney nvidia amd intel samsung xiaomi oppo vivo huawei realme lenovo asus acer dell msi logitech razer steam valorant roblox minecraft fortnite genshin pubg rov dota csgo emoji gif meme selfie vlog podcast webinar login logout signup username password email inbox spam bot chatbot api backend frontend devops fullstack dev prod repo commit merge rebase debug deploy localhost json yaml csv regex async await npm yarn pnpm webpack vite react vue angular svelte nextjs nuxt django flask laravel spring";
const CAPS: &str = "iPhone iPad iOS macOS eBay iMac iCloud McDonald's YouTube TikTok LinkedIn PowerPoint JavaScript TypeScript GitHub PlayStation pH kWh mRNA dB mAh MHz GHz NASA COVID BTS BNK48 KBank SCB PTT AIS DTAC TRUE OK USB-C";
const DOUBLES: &str = "that that|had had|bye bye|no no|so so|very very|ha ha|go go|is is|the the|เร็ว เร็ว|มาก มาก|ไป ไป|นะ นะ|ค่ะ ค่ะ|ดี ดี";

/// Words that auto mode wrongly fixes today. Every one is a bug; remove a
/// word here once it is left alone.
const KNOWN_AUTO: &[&str] = &[];

/// The words in `items` the bubble suggests fixing at some point while they
/// are typed, and those auto mode fixes on Space.
fn hits(d: &Detector, items: &[&str]) -> (Vec<String>, Vec<String>) {
    let (mut suggested, mut auto) = (Vec::new(), Vec::new());
    for w in items {
        let mut f = Fixer::new();
        (f.slips, f.typos, f.spelling) = (true, true, true);
        let mut typed = String::new();
        for c in w.chars() {
            f.typed(&c.to_string());
            typed.push(c);
            if let Some(s) = f.suggestion(d, &[]) {
                suggested.push(format!("{w} @{typed} -> {} ({:?})", s.edit.insert, s.kind));
                break;
            }
        }
        let mut f = Fixer::new();
        (f.typos, f.spelling) = (true, true);
        f.typed(w);
        if let Some(e) = f.auto(d, " ", |_| false, &[]) {
            auto.push(format!("{w} -> {}", e.insert.trim_end()));
        }
    }
    (suggested, auto)
}

/// (layout, list, words, how many the bubble may suggest fixing)
fn cases() -> Vec<(&'static str, &'static str, Vec<&'static str>, usize)> {
    let words = |s: &'static str| s.split(' ').collect::<Vec<_>>();
    let doubles = DOUBLES.split('|').collect::<Vec<_>>();
    vec![
        ("Kedmanee", "Thai chat", words(THAI_CHAT), 1),
        ("Kedmanee", "English tech", words(EN_WORDS), 3),
        ("Kedmanee", "capitals", words(CAPS), 2),
        ("Kedmanee", "doubles", doubles.clone(), 1),
        ("Pattachote", "Thai chat", words(THAI_CHAT), 1),
        ("Pattachote", "English tech", words(EN_WORDS), 10),
        ("Pattachote", "capitals", words(CAPS), 2),
        ("Pattachote", "doubles", doubles, 1),
    ]
}

fn detector(layout: &str) -> Detector {
    let thai = match layout {
        "Kedmanee" => Layout::kedmanee(),
        _ => Layout::pattachote(),
    };
    Detector::new(&Layout::us_qwerty(), &thai)
}

#[test]
fn chat_and_brands_are_rarely_suggested() {
    let mut over = Vec::new();
    for (layout, list, words, budget) in cases() {
        let (suggested, _) = hits(&detector(layout), &words);
        if suggested.len() > budget {
            over.push(format!(
                "{layout} {list}: {} suggested, budget {budget}\n    {}",
                suggested.len(),
                suggested.join("\n    ")
            ));
        } else if suggested.len() < budget {
            println!(
                "{layout} {list}: {} < budget {budget}; lower it",
                suggested.len()
            );
        }
    }
    assert!(over.is_empty(), "\n{}", over.join("\n"));
}

#[test]
fn auto_mode_leaves_them_alone() {
    let mut new = Vec::new();
    let mut fixed = std::collections::BTreeSet::new();
    for (layout, list, words, _) in cases() {
        let (_, auto) = hits(&detector(layout), &words);
        for a in auto {
            let word = a.split(" -> ").next().unwrap().to_string();
            if KNOWN_AUTO.contains(&word.as_str()) {
                fixed.insert(word);
            } else {
                new.push(format!("{layout} {list}: {a}"));
            }
        }
    }
    assert!(
        new.is_empty(),
        "auto mode fixes words typed on purpose:\n{}",
        new.join("\n")
    );
    let gone: Vec<_> = KNOWN_AUTO.iter().filter(|w| !fixed.contains(**w)).collect();
    assert!(
        gone.is_empty(),
        "no longer fixed, remove from KNOWN_AUTO: {gone:?}"
    );
}
