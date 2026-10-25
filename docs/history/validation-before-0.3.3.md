# 0.3.3より前の検証記録

各節の記述・テスト件数・未確認事項は、その検証時点のものです。「映像出力は未対応」「国旗がJPになる」といった当時の結果を、現行仕様として参照しないでください。[現在の検証状況](../validation.md)と[絵文字の設定と制約](../emoji.md)を参照してください。

## 2026-10-25: SVG専用aux2の試作

`comfont-svg-plugin` を追加しました。独立した `comfont_svg.aux2` がLuaモジュール `comfont_svg` を登録し、SVG XMLまたはフォント内の1書記素を指定サイズの透明PNGとして返します。本文の自動置換や混在レイアウトは扱いません。APIと制限は[SVG aux2仕様](../svg-aux2.md)に記載しています。

SVGの16・64・128pxでの透明背景・半透明色、PNG往復、要求とサイズによるキャッシュ、固定width/heightを持つSVGのベクトル再描画、不正XMLと寸法の拒否を検証しました。Twitter Color Emojiの顔・ハート・太陽・肌色・ZWJ・国旗について、同じ1書記素APIを32・96・192pxで実行し、カラー画素と字送り・ベースラインを確認しました。

`cargo test --workspace --all-features --offline --locked` は186件成功、3件ignoreです。Twitter専用のignoreテストを別途実行して1件成功しました。SVG aux2とmodule-pluginのClippy（全ターゲット・全機能、`-D warnings`）、fmt、diff checkも成功しました。

専用AviUtl2 v2.1.14環境 `target/comfont-svg-verify-20261024` へdebug aux2とサンプルobj2を配置しました。32・96・192pxの国旗とSVGを並べるプロジェクトは `tests/aviutl2/svg-raster-verification.aup2` です。ユーザーによる信頼確認後、Luaから別aux2を呼び、返されたPNGがサイズ別にカラー表示されることを確認しました。国旗はJPの文字ではなく旗の絵柄です。

さらにRGB24bitのAVIへ1280×720 / 30fps / 1秒を書き出しました。ffprobeで30フレームを確認し、取り出したフレームでも3サイズの国旗とSVGのカラー表示を確認しました。全30フレームの画素ハッシュが一致しました。この確認は専用カスタムオブジェクトによる画像表示で、通常の「描画@comfont」の混在本文経路を変更するものではありません。

証跡は専用環境内の `svg-raster-preview.png`、`svg-raster-proof.avi`、`svg-raster-export-frame.png`、`svg-raster-proof.framemd5`、`verification-status.json` です。検証したaux2のSHA-256は `13513AF530F13DAD03FA16C816FF34A5925AD755E28982556B9A11E1401E7A00` です。

## 2026-10-24: Twitter Color EmojiのSVG描画

COLR/CPALだけを要求していたカラー描画を、`IDWriteFactory4::TranslateColorGlyphRun` によるSVG-in-OpenTypeの検出にも対応させました。SVGランは専用のDirect2Dコンテキスト（WARP）で描画し、コマンドリストから実際の描画範囲を求めて字形ガイドへ反映します。読み戻したpremultiplied BGRAを既存のプレビュー合成用straight BGRAへ変換します。動画出力のカラーフォント制限は変更していません。

インストール済みの `TwitterColorEmoji-SVGinOT.ttf` を指定した専用テストを実行し、顔・ハート・太陽・肌色付き・ZWJ絵文字・国旗でSVGランとカラー画素を確認しました。水平125%・垂直75%・ベースライン補正、ガイドON/OFFの画素一致と描画画素の境界内包含も確認しました。このフォントのcmapには数字 `1` がなく、キーキャップは別フォントへのフォールバックになるため、SVGランの検証対象からは除外しています。

専用のAviUtl2 v2.1.14（`target/comfont-twitter-preview-verify-20261024`）へ最新debugビルドを配置し、絵文字プリセットにTwitter Color Emojiを指定して `日本語 ABC 😀 ❤️ ☀️ 👍🏽 👩‍💻 🇯🇵 1️⃣` を実機表示しました。顔・ハート・太陽・肌色付き・ZWJ絵文字をカラー表示し、`🇯🇵` も旗の絵柄になりました。ガイドON/OFFでも表示を維持しています。キーキャップはこの環境では白黒の代替字形でした。

配置したプラグインのSHA-256は `9C293A10F754CCF9D29CCE1CE60C2B58F75D50C3DD36B6082DFC3D40356FF683` でビルド元と一致します。証跡は専用環境内の `twitter-svg-preview-guides-off.png`、`twitter-svg-preview-guides-on.png`、`verification-status.json` です。

検証: ワークスペースのテスト183件成功（通常実行では3件無視）、Twitter SVG専用テストを別途明示実行して1件成功、fmt・moduleのClippy成功。ビットマップ形式、COLR v1のペイントツリーは未対応です。SVGの表現範囲はWindowsのDirect2Dの対応範囲に従います。

## 2026-10-24: 絵文字の設定プレビュー

共通描画処理がカラーフォントを拒否し、設定プレビューでも表示を中断していた箇所を修正しました。設定プレビューではDirectWriteの `TranslateColorGlyphRun` でCOLR/CPALレイヤーを取得し、パレット色・透明度・レイヤー原点を保持して合成します。カラー情報のないランは通常の文字色で描画し、グリフ境界には全レイヤーの外接矩形を使います。

Segoe UI Emojiを使い、英数字との混在、ハート・肌色・ZWJ絵文字、Yu Gothic UIからの自動フォールバック、縦横倍率・ベースライン補正、ガイドON/OFFでの画素一致と全描画画素の境界内包含を単体テストで確認しました。入力欄のフォールバックにもSegoe UI Emojiを追加しています。

同日、専用のポータブルAviUtl2 v2.1.14（`target/comfont-emoji-preview-verify-20261024`）へ最新のdebugビルドを配置し、設定画面の実機確認を実施しました。配置した `data/Plugin/comfont.aux2` とビルド元DLLのSHA-256は一致（`9F59826640BBD9B8027BF3FDB38E98EC433C0AD9BC79157489ED2F62EAD73EFE`）。普段使いのAviUtl2へはプラグインやプロファイルを配置していません。

| 実機での操作・条件 | 結果 |
|---|---|
| `日本語 ABC 😀 ❤️ ☀️ 👍🏽 👩‍💻 🇯🇵 1️⃣` をプレビュー本文に入力 | 日本語・英数字と絵文字が混在して表示。顔・ハート・肌色・ZWJ絵文字をカラー表示 |
| 「絵文字（Unicode 18.0）」を検索して追加 | 154範囲の1行として追加できることを確認 |
| 絵文字プリセットにSegoe UI Emojiを入力し適用 | 太陽・キーキャップもカラー表示。ほかの文字も引き続き表示 |
| ガイドON/OFF | 両方でカラー表示を維持。ON時にグリフ境界・字枠・ベースラインを表示 |
| 垂直75%・水平125%・ベースライン20%を適用 | 変形・位置補正後もカラー表示を維持し、ガイド境界も追従 |
| 検証用プロファイルを保存 | 専用環境のJSONへ154範囲・フォント名・倍率・補正値が保存されることを確認 |

証跡は専用環境内の `emoji-preview-guides-off.png`、`emoji-preview-scaled-guides-on.png`、`verification-status.json`、`data/compositefont/profiles.json` です。

この初回確認では `🇯🇵` は国旗の絵柄にならず `JP` と表示されました。また、Yu Gothic UIのままでは太陽が単色になり、Segoe UI Emojiの明示指定でカラーになります。プリセットのコードポイント網羅性と、選択フォントでのカラー字形・合成シーケンスの対応は別条件です。この確認時点ではSVGは対応前でした（追加対応の結果は上記）。動画出力のカラーフォント対応範囲は変更しておらず、今回の実機確認対象は設定プレビューです。

以下は以前のリリース検証記録です。

更新日: 2026-10-19。製品はWindows / aviutl2-rs 0.50.0、必要なAviUtl2はv2.1.14以降です。

## 2026-10-19時点の製品コード

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

この結果はSDK経路を実証したもので、今回統合したプロファイル連携の動作確認ではありません。統合版はビルド済みですが、実機ではプロファイル反映、保存本文、書き出し、フィルタ順序、実書体と負荷を確認する必要があります。詳細は[本文保持の標準描画経路](../native-text-path.md)を参照してください。

## 既存経路

PSDToolKit字幕objectと手動スニペットのLua API結合は、今回のLuaJIT模擬テストで確認しました。実際のPSDToolKitを用いたAviUtl2上の回帰試験は未実施です。

過去の専用ホストでは効果追加時のハング解消、左寄せプレビュー、目アイコン、フォント検索を確認しました。UD新ゴ、実際のPSDToolKit、以前のDirectWrite出力との輪郭・縁取り比較、DPI、繰り返し開閉、再生・書き出しと負荷は引き続き確認対象です。

## 2026-10-19時点のリリース前確認

今回のレビュー対象は `release/vandojapan.comfont-v0.3.2-beta-antialias-fix.au2pkg.zip` です。βを含まない手動作成パッケージと `release/catalog.json` は対象外です。βパッケージの同梱ファイル8件を確認し、プラグイン・anm2・字幕object・連携スニペット・ライセンスの7件が現在のビルド・ソースとSHA-256で一致しました。

- 製品統合版をAviUtl2で確認する。標準テキストと `DIN,fifififififififfflflfl`（サイズ300・473）について、強制ON/OFF、AA「標準／滑らか／なし」、プレビューと書き出し、保存本文、プロファイル反映を比較する。ONで対応外の装飾・タグを使った場合に標準描画へ戻ることも確認する。
- リガチャONはオブジェクト全体を画像化する試験機能。標準描画との輪郭・位置・字間の差と性能を実機で確認するまでは、品質回復済みとは判定しない。
