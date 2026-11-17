use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey,
    UnregisterHotKey,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HotKey {
    pub mods: u32,
    pub vk: u32,
}

/// "Ctrl+Alt+V" のような文字列を解釈する。大文字小文字は区別しない。
pub fn parse(text: &str) -> Result<HotKey, String> {
    let mut mods = 0u32;
    let mut vk = None;
    for part in text.split('+').map(str::trim) {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => mods |= MOD_CONTROL.0,
            "alt" => mods |= MOD_ALT.0,
            "shift" => mods |= MOD_SHIFT.0,
            "win" => mods |= MOD_WIN.0,
            "" => return Err(format!("ホットキー \"{text}\" の書き方が正しくありません")),
            _ => {
                if vk.is_some() {
                    return Err(format!("ホットキー \"{text}\" にキーが2つ以上あります"));
                }
                vk = Some(parse_key(part)?);
            }
        }
    }
    let vk = vk.ok_or_else(|| format!("ホットキー \"{text}\" にキーがありません"))?;
    if mods == 0 {
        return Err(format!(
            "ホットキー \"{text}\" には Ctrl / Alt / Shift / Win のどれかが必要です"
        ));
    }
    Ok(HotKey { mods, vk })
}

/// 名前のあるキー（仮想キーコード）。`parse_key` と `Display` が同じ表を使う。
const NAMED_KEYS: [(&str, u32); 2] = [("Space", 0x20), ("Insert", 0x2D)];
/// F1〜F24 の仮想キーコードの範囲（F1 の値と、キーの数）。
const VK_F1: u32 = 0x70;
const F_KEYS: u32 = 24;

fn parse_key(key: &str) -> Result<u32, String> {
    let upper = key.to_ascii_uppercase();
    let bytes = upper.as_bytes();
    if bytes.len() == 1 && (bytes[0].is_ascii_uppercase() || bytes[0].is_ascii_digit()) {
        return Ok(bytes[0] as u32);
    }
    if let Some(n) = upper.strip_prefix('F').and_then(|n| n.parse::<u32>().ok())
        && (1..=F_KEYS).contains(&n)
    {
        return Ok(VK_F1 + n - 1);
    }
    NAMED_KEYS
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(key))
        .map(|&(_, vk)| vk)
        .ok_or_else(|| format!("不明なキー \"{key}\" です"))
}

/// 表示用の決まった書き方（"Ctrl+Alt+Shift+Win+キー" の順。キーは `parse_key` が受け付ける名前）。
/// ツールチップと通知は設定の書き方ではなくこれを使う（台帳 D42）。parse すると同じホットキーに戻る。
impl std::fmt::Display for HotKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names = [
            (MOD_CONTROL, "Ctrl"),
            (MOD_ALT, "Alt"),
            (MOD_SHIFT, "Shift"),
            (MOD_WIN, "Win"),
        ];
        for (flag, name) in names {
            if self.mods & flag.0 != 0 {
                write!(f, "{name}+")?;
            }
        }
        if let Some((name, _)) = NAMED_KEYS.iter().find(|(_, vk)| *vk == self.vk) {
            return f.write_str(name);
        }
        match self.vk {
            vk if (VK_F1..VK_F1 + F_KEYS).contains(&vk) => write!(f, "F{}", vk - VK_F1 + 1),
            // A〜Z・0〜9 は仮想キーコードが文字コードと同じ
            vk => match char::from_u32(vk) {
                Some(c) => write!(f, "{c}"),
                None => write!(f, "0x{vk:02X}"),
            },
        }
    }
}

pub fn register(hwnd: HWND, id: i32, hotkey: HotKey) -> windows::core::Result<()> {
    // SAFETY: 値だけを渡し、ポインタを渡さない。hwnd が無効なら失敗が返るだけ。
    unsafe {
        RegisterHotKey(
            Some(hwnd),
            id,
            HOT_KEY_MODIFIERS(hotkey.mods | MOD_NOREPEAT.0),
            hotkey.vk,
        )
    }
}

pub fn unregister(hwnd: HWND, id: i32) {
    // SAFETY: 値だけを渡す。登録していない id なら失敗が返るだけで、それは無視してよい。
    unsafe {
        let _ = UnregisterHotKey(Some(hwnd), id);
    }
}

/// 最初に登録するときの ID。
const FIRST_ID: i32 = 1;
/// 設定を読み直すとき、新しいキーを今のキーと別の ID で先に登録するために使う（台帳 D36）。
const SPARE_ID: i32 = 2;

/// 登録して使っているホットキー。
#[derive(Debug, Clone, PartialEq)]
pub struct Active {
    pub id: i32,
    pub key: HotKey,
}

/// 設定を反映したあとのホットキー。
#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// 設定どおりのキーを使っている
    Registered(String),
    /// 設定に誤りがあったので、今のキーをそのまま使っている
    Kept(String),
    /// 設定どおりのキーを登録できなかった。kept は代わりに使い続けているキー
    Failed {
        wanted: String,
        kept: Option<String>,
    },
}

impl Outcome {
    pub fn is_registered(&self) -> bool {
        matches!(self, Outcome::Registered(_))
    }
}

/// 設定から決めたホットキー key に切り替える。戻り値は切り替えたあとに使っているホットキーと、その結果。
/// 新しいキーは今のキーと別の ID で登録し、登録できたときだけ今のキーを外すので、登録できなければ今のキーが残る。
/// 設定に誤りがある（`has_warning`）ときも、今のキーがあればそれを使い続ける（台帳 D36）。
pub fn switch(
    current: Option<Active>,
    key: HotKey,
    has_warning: bool,
    register: impl FnOnce(i32, HotKey) -> bool,
    unregister: impl FnOnce(i32),
) -> (Option<Active>, Outcome) {
    match current {
        Some(current) if has_warning => {
            let outcome = Outcome::Kept(current.key.to_string());
            (Some(current), outcome)
        }
        // 同じキーを別の ID で登録しようとすると RegisterHotKey が失敗するので、そのまま使う
        Some(current) if current.key == key => {
            let outcome = Outcome::Registered(key.to_string());
            (Some(current), outcome)
        }
        current => {
            let id = match &current {
                Some(c) if c.id == FIRST_ID => SPARE_ID,
                _ => FIRST_ID,
            };
            if register(id, key) {
                if let Some(old) = current {
                    unregister(old.id);
                }
                (
                    Some(Active { id, key }),
                    Outcome::Registered(key.to_string()),
                )
            } else {
                let kept = current.as_ref().map(|c| c.key.to_string());
                let outcome = Outcome::Failed {
                    wanted: key.to_string(),
                    kept,
                };
                (current, outcome)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_hotkey() {
        assert_eq!(
            parse("Ctrl+Alt+V"),
            Ok(HotKey {
                mods: MOD_CONTROL.0 | MOD_ALT.0,
                vk: 'V' as u32
            })
        );
    }

    #[test]
    fn accepts_control_alias_and_insert_key() {
        assert_eq!(
            parse("Control+Insert"),
            Ok(HotKey {
                mods: MOD_CONTROL.0,
                vk: 0x2D, // VK_INSERT
            })
        );
    }

    #[test]
    fn ignores_case_and_spaces() {
        assert_eq!(
            parse(" ctrl + shift + e "),
            Ok(HotKey {
                mods: MOD_CONTROL.0 | MOD_SHIFT.0,
                vk: 'E' as u32
            })
        );
    }

    #[test]
    fn parses_function_and_digit_keys() {
        assert_eq!(parse("Win+F12").unwrap().vk, 0x7B);
        assert_eq!(parse("Alt+1").unwrap().vk, '1' as u32);
        assert_eq!(parse("Ctrl+Space").unwrap().vk, 0x20);
    }

    // 台帳 D42: 表示は設定の書き方（空白・大文字小文字・修飾キーの順）によらず、決まった書き方にする
    #[test]
    fn displays_in_canonical_form() {
        assert_eq!(
            parse(" ctrl + shift + e ").unwrap().to_string(),
            "Ctrl+Shift+E"
        );
        assert_eq!(
            parse("shift+win+alt+control+f12").unwrap().to_string(),
            "Ctrl+Alt+Shift+Win+F12"
        );
        assert_eq!(parse("ALT+space").unwrap().to_string(), "Alt+Space");
        assert_eq!(parse("control+insert").unwrap().to_string(), "Ctrl+Insert");
        assert_eq!(parse("Ctrl+7").unwrap().to_string(), "Ctrl+7");
    }

    #[test]
    fn display_parses_back_to_the_same_hotkey() {
        let keys = ('A'..='Z')
            .chain('0'..='9')
            .map(String::from)
            .chain((1..=24).map(|n| format!("F{n}")))
            .chain(["Space".into(), "Insert".into()]);
        for key in keys {
            let hotkey = parse(&format!("Ctrl+Win+{key}")).unwrap();
            assert_eq!(parse(&hotkey.to_string()), Ok(hotkey), "{key}");
        }
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse("").is_err());
        assert!(parse("V").is_err(), "修飾キーなし");
        assert!(parse("Ctrl+Alt").is_err(), "キーなし");
        assert!(parse("Ctrl+A+B").is_err(), "キーが2つ");
        assert!(parse("Ctrl++V").is_err(), "空の部分");
        assert!(parse("Ctrl+F25").is_err());
        assert!(parse("Ctrl+Hoge").is_err());
    }

    /// 設定に誤りがあるとき、main の `choose_hotkey` が代わりに返してくる既定のキー
    const DEFAULT: &str = "Ctrl+Alt+V";

    fn key(text: &str) -> HotKey {
        parse(text).unwrap()
    }

    fn active(id: i32, text: &str) -> Active {
        Active { id, key: key(text) }
    }

    /// switch を、登録が成功するかどうかを決めて呼ぶ。登録・解除した ID も返す。
    fn try_switch(
        current: Option<Active>,
        text: &str,
        has_warning: bool,
        register_ok: bool,
    ) -> (Option<Active>, Outcome, Vec<i32>, Vec<i32>) {
        let (mut registered, mut unregistered) = (Vec::new(), Vec::new());
        let (next, outcome) = super::switch(
            current,
            key(text),
            has_warning,
            |id, _| {
                registered.push(id);
                register_ok
            },
            |id| unregistered.push(id),
        );
        (next, outcome, registered, unregistered)
    }

    #[test]
    fn first_hotkey_is_registered() {
        let (next, outcome, registered, _) = try_switch(None, "Ctrl+Alt+V", false, true);
        assert_eq!(next, Some(active(FIRST_ID, "Ctrl+Alt+V")));
        assert_eq!(outcome, Outcome::Registered("Ctrl+Alt+V".into()));
        assert_eq!(registered, [FIRST_ID]);
    }

    // 台帳 D36: 新しいキーを別の ID で登録できてから、今のキーを外す
    #[test]
    fn new_hotkey_is_registered_before_the_current_one_is_released() {
        let current = active(FIRST_ID, "Ctrl+Alt+V");
        let (next, outcome, registered, unregistered) =
            try_switch(Some(current), "Ctrl+Shift+E", false, true);
        assert_eq!(next, Some(active(SPARE_ID, "Ctrl+Shift+E")));
        assert_eq!(outcome, Outcome::Registered("Ctrl+Shift+E".into()));
        assert_eq!(registered, [SPARE_ID]);
        assert_eq!(unregistered, [FIRST_ID]);
    }

    #[test]
    fn current_hotkey_stays_when_the_new_one_cannot_be_registered() {
        let current = active(FIRST_ID, "Ctrl+Alt+V");
        let (next, outcome, _, unregistered) =
            try_switch(Some(current.clone()), "Ctrl+Shift+E", false, false);
        assert_eq!(next, Some(current));
        assert_eq!(
            outcome,
            Outcome::Failed {
                wanted: "Ctrl+Shift+E".into(),
                kept: Some("Ctrl+Alt+V".into())
            }
        );
        assert!(unregistered.is_empty(), "今のキーを外さない");
    }

    #[test]
    fn current_hotkey_stays_when_the_config_has_an_error() {
        let current = active(SPARE_ID, "Ctrl+Shift+E");
        // 設定に誤りがあるときは、choose_hotkey が既定のキーを返してくる
        let (next, outcome, registered, unregistered) =
            try_switch(Some(current.clone()), DEFAULT, true, true);
        assert_eq!(next, Some(current));
        assert_eq!(outcome, Outcome::Kept("Ctrl+Shift+E".into()));
        assert!(registered.is_empty() && unregistered.is_empty());
    }

    #[test]
    fn config_error_at_startup_still_registers_the_default() {
        let (next, _, registered, _) = try_switch(None, DEFAULT, true, true);
        assert_eq!(next, Some(active(FIRST_ID, DEFAULT)));
        assert_eq!(registered, [FIRST_ID]);
    }

    // 台帳 D36: 同じキーのまま読み直したら、登録し直さない
    #[test]
    fn same_key_is_not_registered_again() {
        let current = active(1, "Ctrl+Shift+E");
        let (next, outcome, registered, unregistered) =
            try_switch(Some(current.clone()), "ctrl + shift + e", false, false);
        assert_eq!(next, Some(current));
        assert_eq!(outcome, Outcome::Registered("Ctrl+Shift+E".into()));
        assert_eq!(registered, Vec::<i32>::new());
        assert_eq!(unregistered, Vec::<i32>::new());
    }

    #[test]
    fn every_key_name_round_trips() {
        let names = ('A'..='Z')
            .chain('0'..='9')
            .map(String::from)
            .chain((1..=24).map(|n| format!("F{n}")))
            .chain(["Space", "Insert"].map(String::from));
        for name in names {
            let text = format!("Ctrl+{name}");
            assert_eq!(parse(&text).unwrap().to_string(), text);
        }
    }

    // 台帳 D44: 誤りの文のキー名は、書いたとおりに出す
    #[test]
    fn unknown_key_is_quoted_as_written() {
        assert!(parse("Ctrl+Nope").unwrap_err().contains("\"Nope\""));
    }
}
