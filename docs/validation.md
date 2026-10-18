# 検証状況

更新日: 2026-10-19。製品はWindows / aviutl2-rs 0.50.0、必要なAviUtl2はv2.1.14以降です。

## 製品コード

| 確認 | 結果 |
|---|---|
| `cargo fmt --all -- --check` | 成功 |
| `cargo check --workspace --all-features --offline --locked` | 成功 |
| `cargo test --workspace --all-features --offline --locked` | 成功（core 19・editor 59・module 90。要LuaJITの1件は通常実行では無視） |
| `cargo test -p compositefont-module-plugin --test lua_pipeline --offline --locked -- --ignored` | 検証用ホストのLuaJITを指定して成功。模擬APIを現行anm2に合わせて更新 |
| `cargo clippy --workspace --all-targets --all-features --offline --locked -- -D warnings` | 成功 |
| `cargo build -p compositefont-editor-plugin --release --offline --locked` | 成功 |
| 製品統合版のAviUtl2実機確認 | 未実施 |

標準テキストの装飾は、aux2内の非表示フィルタから一時パラメータで標準「テキスト」を呼び直す経路へ接続しました。保存本文は読み取り専用です。Lua本文、未対応タグ、複数テキスト入力等は通常表示へ戻します。

SDK 0.50.0でcore 19・editor 59・module 90の単体テストを通しています。ON時の合字設定が保存済みの解除指定より優先されること、アンチエイリアス3方式の画素差と設定値の読み取り、512を超える文字サイズ、幅4096ピクセルを超える行での合字組版を単体テストで確認しました。LuaJITテストはスクリプトと模擬ホストの接続確認であり、AviUtl2上の描画品質や書き出しの検証ではありません。

ZWNJ挿入は標準出力・プレビューから撤去済みです。通常の標準テキスト出力ではOpenTypeの任意機能を個別に切り替えられず、プロファイル内のリガチャ設定は設定プレビュー専用です。「リガチャを強制ON」の試験経路ではオブジェクト全体をDirectWriteで画像化し、任意の合字機能を要求します。これは標準描画と画質が異なる可能性があり、実機での比較は未実施です。

## 描画経路の実機試作

専用ホスト `target/comfont-native-text-verify-20261018`（AviUtl2 v2.1.14）で、以前の固定値試作を確認しました。

- anm2がaux2内フィルタを呼び、標準「テキスト」を `exec_effect` で実行。
- 保存本文 `原文 ABC ffi` は変更されない。
- 出力は300×112で、標準テキストへ同じ制御タグを直接書いた参照とRGBA画素が完全一致。
- 独自ビットマップ生成・画素アップロードなし。画素取得は比較用だけに使用。

この結果はSDK経路を実証したもので、今回統合したプロファイル連携の動作確認ではありません。統合版はビルド済みですが、実機ではプロファイル反映、保存本文、書き出し、フィルタ順序、実書体と負荷を確認する必要があります。詳細は[本文保持の標準描画経路](native-text-path.md)を参照してください。

## 既存経路

PSDToolKit字幕objectと手動スニペットのLua API結合は、今回のLuaJIT模擬テストで確認しました。実際のPSDToolKitを用いたAviUtl2上の回帰試験は未実施です。

過去の専用ホストでは効果追加時のハング解消、左寄せプレビュー、目アイコン、フォント検索を確認しました。UD新ゴ、実際のPSDToolKit、以前のDirectWrite出力との輪郭・縁取り比較、DPI、繰り返し開閉、再生・書き出しと負荷は引き続き確認対象です。

## リリース前に必要な確認

今回のレビュー対象は `release/vandojapan.comfont-v0.3.2-beta-antialias-fix.au2pkg.zip` です。βを含まない手動作成パッケージと `release/catalog.json` は対象外です。βパッケージの同梱ファイル8件を確認し、プラグイン・anm2・字幕object・連携スニペット・ライセンスの7件が現在のビルド・ソースとSHA-256で一致しました。

- 製品統合版をAviUtl2で確認する。標準テキストと `DIN,fifififififififfflflfl`（サイズ300・473）について、強制ON/OFF、AA「標準／滑らか／なし」、プレビューと書き出し、保存本文、プロファイル反映を比較する。ONで対応外の装飾・タグを使った場合に標準描画へ戻ることも確認する。
- リガチャONはオブジェクト全体を画像化する試験機能。標準描画との輪郭・位置・字間の差と性能を実機で確認するまでは、品質回復済みとは判定しない。
