// assets/clipedit.manifest（台帳 D5）と assets/clipedit.ico、版の情報（台帳 D39）を exe に埋め込み、
// DLL の探し方を制限する（監査 S9）。MSVC のリンカーでだけ行う。
use std::path::Path;

/// エクスプローラーは、いちばん小さい番号のアイコングループを exe のアイコンにする。
const ICON_GROUP_ID: u16 = 1;
const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
const RT_VERSION: u16 = 16;
const VS_FF_PRERELEASE: u32 = 0x2;
/// 版の情報の言語（日本語）と文字コード（UTF-16）。
const LANG_JA: u16 = 0x0411;
const CODEPAGE_UNICODE: u16 = 1200;

fn main() {
    println!("cargo:rerun-if-changed=assets/clipedit.manifest");
    println!("cargo:rerun-if-changed=assets/clipedit.ico");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=LICENSE");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        return;
    }
    // 監査 S9: 起動時に読み込む DLL を System32 からだけ探す（0x800 = LOAD_LIBRARY_SEARCH_SYSTEM32）。
    // 動いている途中で読み込むものは、main の最初の SetDefaultDllDirectories が受け持つ
    println!("cargo:rustc-link-arg-bins=/DEPENDENTLOADFLAG:0x800");
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
    println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
    println!(
        "cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}",
        assets.join("clipedit.manifest").display()
    );

    let ico = std::fs::read(assets.join("clipedit.ico")).expect("assets/clipedit.ico を読めない");
    let res = Path::new(&std::env::var("OUT_DIR").unwrap()).join("clipedit.res");
    let license = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("LICENSE"))
        .expect("LICENSE を読めない");
    let copyright = license
        .lines()
        .find(|l| l.starts_with("Copyright"))
        .expect("LICENSE に Copyright の行がない");
    let mut contents = icon_res(&ico);
    contents.extend_from_slice(&version_res(env!("CARGO_PKG_VERSION"), copyright));
    std::fs::write(&res, contents).expect("clipedit.res を書けない");
    // link.exe は .res をそのまま入力に取れるので、rc.exe は使わない
    println!("cargo:rustc-link-arg-bins={}", res.display());
}

/// .ico の画像を1枚ずつ `RT_ICON` にし、それをまとめる `RT_GROUP_ICON` を付けた .res を作る。
fn icon_res(ico: &[u8]) -> Vec<u8> {
    let u16_at = |at: usize| u16::from_le_bytes([ico[at], ico[at + 1]]);
    let u32_at = |at: usize| u32::from_le_bytes(ico[at..at + 4].try_into().unwrap());
    let count = u16_at(4);
    assert!(
        u16_at(2) == 1 && count > 0,
        "clipedit.ico がアイコンの形式でない"
    );

    // .res の先頭には、32 ビット形式であることを示す空の項目を置く
    let mut res = Vec::new();
    push_resource(&mut res, 0, 0, 0, &[]);
    let mut group = Vec::new();
    group.extend_from_slice(&[0, 0, 1, 0]);
    group.extend_from_slice(&count.to_le_bytes());
    for i in 0..count {
        let entry = 6 + 16 * usize::from(i);
        let len = u32_at(entry + 8) as usize;
        let offset = u32_at(entry + 12) as usize;
        let id = i + 1;
        push_resource(&mut res, RT_ICON, id, 0x1010, &ico[offset..offset + len]);
        // .ico の項目（幅〜バイト数の 12 バイト）はそのまま使い、データの位置の代わりに番号を書く
        group.extend_from_slice(&ico[entry..entry + 12]);
        group.extend_from_slice(&id.to_le_bytes());
    }
    push_resource(&mut res, RT_GROUP_ICON, ICON_GROUP_ID, 0x1030, &group);
    res
}

/// 種類と名前を番号で持つ項目を1つ足す（ヘッダーは 32 バイト、データは 4 バイト境界にそろえる）。
fn push_resource(res: &mut Vec<u8>, kind: u16, name: u16, flags: u16, data: &[u8]) {
    let data_size = u32::try_from(data.len()).unwrap();
    res.extend_from_slice(&data_size.to_le_bytes());
    res.extend_from_slice(&32u32.to_le_bytes());
    for ordinal in [kind, name] {
        res.extend_from_slice(&0xFFFFu16.to_le_bytes());
        res.extend_from_slice(&ordinal.to_le_bytes());
    }
    res.extend_from_slice(&0u32.to_le_bytes()); // DataVersion
    res.extend_from_slice(&flags.to_le_bytes()); // MemoryFlags
    res.extend_from_slice(&0u16.to_le_bytes()); // LanguageId（言語を問わない）
    res.extend_from_slice(&0u32.to_le_bytes()); // Version
    res.extend_from_slice(&0u32.to_le_bytes()); // Characteristics
    res.extend_from_slice(data);
    res.resize(res.len().next_multiple_of(4), 0);
}

/// `RT_VERSION` の項目を1つ作る（.res の先頭の空の項目は含まない）。エクスプローラーのプロパティの「詳細」に出る。
/// version は Cargo.toml の版（"0.3.6" のような 3 つの数）。
fn version_res(version: &str, copyright: &str) -> Vec<u8> {
    let parts: Vec<u32> = version
        .split(['.', '-', '+'])
        .take(3)
        .map(|p| p.parse().expect("版の数を読めない"))
        .collect();
    let [major, minor, patch] = parts[..] else {
        panic!("版は 3 つの数でない: {version}")
    };
    let ms = (major << 16) | minor;
    let ls = patch << 16;
    // 試験版（"0.4.0-beta.1" など）は、数の版が正式版と同じになるので、VS_FF_PRERELEASE で見分けられるようにする（台帳 D73）
    let flags: u32 = if version.contains('-') {
        VS_FF_PRERELEASE
    } else {
        0
    };
    // VS_FIXEDFILEINFO（DWORD が 13 個）
    let fixed: Vec<u8> = [
        0xFEEF_04BD, // dwSignature
        0x0001_0000, // dwStrucVersion
        ms,          // dwFileVersionMS
        ls,          // dwFileVersionLS
        ms,          // dwProductVersionMS
        ls,          // dwProductVersionLS
        0x3F,        // dwFileFlagsMask
        flags,       // dwFileFlags
        0x0004_0004, // dwFileOS = VOS_NT_WINDOWS32
        1,           // dwFileType = VFT_APP
        0,           // dwFileSubtype
        0,           // dwFileDateMS
        0,           // dwFileDateLS
    ]
    .iter()
    .flat_map(|d: &u32| d.to_le_bytes())
    .collect();

    let strings = join_aligned(
        [
            ("FileDescription", "ClipEdit"),
            ("FileVersion", version),
            ("InternalName", "clipedit"),
            ("LegalCopyright", copyright),
            ("OriginalFilename", "clipedit.exe"),
            ("ProductName", "ClipEdit"),
            ("ProductVersion", version),
        ]
        .iter()
        .map(|(key, value)| {
            let text = utf16z(value);
            let chars = u16::try_from(text.len() / 2).unwrap();
            version_block(key, chars, true, &text, &[])
        }),
    );
    let table = version_block(
        &format!("{LANG_JA:04X}{CODEPAGE_UNICODE:04X}"),
        0,
        true,
        &[],
        &strings,
    );
    let string_info = version_block("StringFileInfo", 0, true, &[], &table);
    let translation: Vec<u8> = [LANG_JA, CODEPAGE_UNICODE]
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    let var = version_block("Translation", 4, false, &translation, &[]);
    let var_info = version_block("VarFileInfo", 0, true, &[], &var);
    let children = join_aligned([string_info, var_info]);
    let root = version_block("VS_VERSION_INFO", 52, false, &fixed, &children);

    let mut res = Vec::new();
    push_resource(&mut res, RT_VERSION, 1, 0x0030, &root);
    res
}

/// 版の情報の入れ物を1つ作る: wLength・wValueLength・wType・キー（0 終端の UTF-16）・値・子。
/// キーのあとと値のあとは 4 バイト境界にそろえる。wLength は最後の子の後ろのそろえを含めない。
fn version_block(
    key: &str,
    value_length: u16,
    text: bool,
    value: &[u8],
    children: &[u8],
) -> Vec<u8> {
    let mut block = vec![0, 0];
    block.extend_from_slice(&value_length.to_le_bytes());
    block.extend_from_slice(&u16::from(text).to_le_bytes());
    block.extend_from_slice(&utf16z(key));
    block.resize(block.len().next_multiple_of(4), 0);
    block.extend_from_slice(value);
    if !children.is_empty() {
        block.resize(block.len().next_multiple_of(4), 0);
        block.extend_from_slice(children);
    }
    let length = u16::try_from(block.len()).expect("版の情報が 64KB を超える");
    block[..2].copy_from_slice(&length.to_le_bytes());
    block
}

/// 子の入れ物を並べる。それぞれの始まりを 4 バイト境界にそろえる。
fn join_aligned(blocks: impl IntoIterator<Item = Vec<u8>>) -> Vec<u8> {
    let mut joined = Vec::new();
    for block in blocks {
        joined.resize(joined.len().next_multiple_of(4), 0);
        joined.extend_from_slice(&block);
    }
    joined
}

/// 0 終端の UTF-16LE のバイト列。
fn utf16z(text: &str) -> Vec<u8> {
    text.encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect()
}
