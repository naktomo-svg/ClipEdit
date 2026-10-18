// assets/clipedit.manifest（台帳 D5）と assets/clipedit.ico を exe に埋め込み、DLL の探し方を制限する（監査 S9）。MSVC のリンカーでだけ行う。
use std::path::Path;

/// エクスプローラーは、いちばん小さい番号のアイコングループを exe のアイコンにする。
const ICON_GROUP_ID: u16 = 1;
const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;

fn main() {
    println!("cargo:rerun-if-changed=assets/clipedit.manifest");
    println!("cargo:rerun-if-changed=assets/clipedit.ico");
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
    std::fs::write(&res, icon_res(&ico)).expect("clipedit.res を書けない");
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
