# 検証状況

このページは0.3.3 βの確認結果です。利用条件は[絵文字の設定と制約](emoji.md)、過去の設定プレビュー・別aux2・以前のリリース記録は[履歴](history/validation-before-0.3.3.md)を参照してください。

## 確認済みと未確認の範囲

| 項目 | 確認状況 |
|---|---|
| 0.3.3 βのreleaseビルド・パッケージ | キーキャップSVG修正版は全機能テスト202件・Twitter専用テスト1件成功、releaseパッケージを作成。過去の版は下記の履歴を参照 |
| 設定プレビューのCOLR/CPAL・SVGカラー描画 | 単体テストと専用ホストで確認。詳細は過去の検証記録 |
| 既存「描画@comfont」でのSVG絵文字合成 | 専用ホストのdebugビルドでプレビュー・AVI出力を確認 |
| 日本・米国・英国の国旗、肌色・ZWJ、日本語との複数行混在 | 上記の同じ条件で確認。全絵文字の実機検証ではない |
| SVGの水平100%・101%・125%・150%・200%、水平125%／垂直75% | 水平倍率修正版のreleaseビルドで、映像プレビュー・AVI書き出しを確認。ベースライン補正は設定プレビューで確認 |
| 単独の数字の除外・キーキャップの下位ルールへの引き渡しと描画 | キーキャップ描画修正版のreleaseビルドで、水平100%／200%のキーキャップを映像プレビュー・AVI出力で確認。COLRと単色の字形全体を合成 |
| Twitter内蔵のキーキャップSVG直接取得 | 実フォントテストと専用ホストのreleaseビルドで12種類、VS16省略形、100%・101%・200%・水平125%／垂直75%、`🔟`、下位への引き渡し、映像プレビュー・AVI出力を確認 |
| 標準の縁取り・ドロップシャドウ・ぼかしとの組み合わせ | comfontの前後、縁取り＋影、入力側の文字装飾を10条件で映像プレビュー・AVI出力確認。効果はcomfontの後段へ配置 |
| アニメーション・中間点・その他のフィルタ、DPI・負荷 | 網羅した実機確認は未実施 |
| リガチャ強制ONとSVG絵文字の併用 | 保証対象外。SVG絵文字はOFFで使用 |
| PSDToolKit字幕object・Lua本文でのSVG自動合成 | 現行の標準テキスト向け自動処理の対象外 |

初回の6行混在本文の実機確認はdebugビルドです。その後、水平倍率修正版については配布パッケージと同じrelease DLLを使い、下記の6条件を実機確認しました。

## 2026-10-25: 縁取り・影・ぼかしとの組み合わせ

専用AviUtl2 v2.1.14で、キーキャップSVG修正版の配布パッケージと同じrelease DLL（SHA-256 `CCAF0D3502BDD498FAD9E1AF66D0FA6EC1055FA5156B9737712268CCE2D3EA63`）を使用しました。実装変更はありません。文字サイズ48、Twitter Color Emojiの水平・垂直100%、リガチャ強制OFFで、`あA 12 #* 1️⃣ #️⃣ 🔟 🇯🇵 👩‍💻 お` を比較しました。

| 行 | 条件（左から処理順） | 映像プレビュー・AVI出力の結果 |
|---|---|---|
| 01 | 描画@comfont | 比較基準。キーキャップ・日本国旗・ZWJをカラー合成 |
| 02 | 描画@comfont → 縁取り | 通常文字と絵文字の輪郭へ桃色の縁を付加 |
| 03 | 縁取り → 描画@comfont | 縁が消え、位置も基準からずれた |
| 04 | 描画@comfont → ドロップシャドウ | 通常文字と絵文字へ水色の影を付加 |
| 05 | ドロップシャドウ → 描画@comfont | 影が消えた |
| 06 | 描画@comfont → ぼかし | 通常文字と絵文字の両方がぼけた |
| 07 | ぼかし → 描画@comfont | ぼかしが消え、位置も基準からずれた |
| 08 | 描画@comfont → 縁取り → ドロップシャドウ | 縁取りと影を同時に適用。合成字形とカラーを維持 |
| 09 | 入力の「縁取り文字」＋描画@comfont | SVG合成の対象外。JPへの分離、数字キーキャップの欠落、ZWJの分離を再現 |
| 10 | 入力の「影付き文字」＋描画@comfont | 行09と同様に標準描画へ戻った |

縁取りはサイズ5・ぼかし0・色 `ff60a0`、影はX/Y各12・濃さ100・拡散0・色 `00c0ff`、ぼかしは範囲5・縦横比0・光の強さ0・サイズ固定OFFです。後段の縁取りと影は画像全体のアルファへ作用し、青いキーキャップの内側の白い数字だけを縁取る処理ではありません。

検証プロジェクトは `target/comfont-scale-verify-20261025/filter-matrix.aup2`。同じディレクトリに `filter-matrix-cases.json`、`filter-matrix-release.avi`、`filter-matrix-release-frame.png`、`filter-matrix-release.framemd5`、`filter-matrix-analysis.json` を保存しました。AVIは1920×1080 / 30fps / RGB24 / 1秒で、全30フレームの画素MD5は `1c5e5e96e295c88f73058b041a23d485` で一致しました。行03・07は基準より左上へ各5pxずれ、位置合わせ後の本文領域の画素は基準と完全一致します。行05は位置合わせなしで基準と一致しました。第三者製フィルタ、動くパラメータ、中間点、DPI・負荷の網羅確認は含みません。

## 2026-10-25: Twitter Color Emoji内蔵のキーキャップSVGを取得

手元のTwitter Color Emoji SVGinOTには、構成文字のcmapがない一方で、12種類すべてのキーキャップSVGが収録されていることを確認しました。実際のフォントファミリ名、postテーブルの字形名、グリフごとのSVG対応を確認してIDを取得する互換処理を追加しました。IDは固定せず、欠けた字形は個別に未対応として扱います。フォントを書き換えたり、追加インストールしたりする処理はありません。

解決したキーキャップは、元のシーケンス全体をDirectWriteのinline objectとして配置し、既存のSVG描画処理を使用します。設定プレビューと画像生成で共有し、映像側では既存のComfont Emoji Markerで合成します。互換SVGが見つかった場合は絵文字行の設定を維持し、見つからず構成文字も欠ける場合は既存の下位ルールへ渡します。単独の数字・`#`・`*` は引き続き対象外です。単独文字U+1F51Fの `🔟` もキーキャップ画像合成の対象にしました。

全機能テスト202件成功・3件ignore。Twitter専用テストを明示実行して1件成功し、インストール済みフォントの12種類×VS16あり／なしを水平100%・101%・200%・水平125%／垂直75%で描画しました。各シーケンスが対応するSVGのグリフIDで描画されること、VS16省略時の画素一致、12種類の画像の相違、フォント本来の字送り、`🔟`、通常数字との混在、絵文字行の倍率維持、設定プレビューと標準描画用の書式解決の一致、13個のマーカースロットへの合成計画を検証しました。破損・切断・重複したpostテーブルの拒否と、一部の字形が欠ける場合の処理も検証しました。Clippy（全ターゲット・全機能、`-D warnings`）、fmt、diff checkは成功しています。

releaseビルドでもTwitter専用テスト1件が成功しました。連続するキーキャップの字間がUTF-16の構成文字数に応じて重複せず、書記素ごとに適用されることも確認しました。

配布物は `release/vandojapan.comfont-v0.3.3-beta-emoji-keycap-svg-fix.au2pkg.zip`（SHA-256 `1950C6D04A4EC005D4ED1ECB9580DF608333CA206796B8467D50D237C8E1AD55`）。同梱・専用ホスト配置・release出力DLLのSHA-256は `CCAF0D3502BDD498FAD9E1AF66D0FA6EC1055FA5156B9737712268CCE2D3EA63` で一致します。テストログは `target/keycap-svg-tests.txt`、`keycap-svg-twitter-test.txt`、`keycap-svg-twitter-release-test.txt`、`keycap-svg-clippy.txt`、`keycap-svg-package.txt` です。

専用AviUtl2 v2.1.14を最終release DLLで再起動し、6行の `target/comfont-scale-verify-20261025/keycap-svg-proof.aup2` で映像プレビューとRGB24 AVIを確認しました。Twitterの12種類と `🔟` を水平100%・101%・200%、水平125%／垂直75%、VS16省略形で表示し、通常の数字・`#`・`*` は等倍を維持しました。Arialの絵文字行から下位のSegoe UI Emojiへ渡した12種類も、分離せずカラーの画像として合成されます。比較行の `🔟` は単独文字であり、構成文字不足による引き渡しの判定とは別にDirectWriteのフォールバックで描画されています。

実機確認中、幅の異なるキーキャップを隙間なく並べると、位置測定用の矩形同士が重なって合成を解除する問題を再現しました。字送りとは別に2～8px幅の細いマーカーを使い、字間・横変形による原点の差を補正しました。通常文字の描画は維持します。回帰テストを追加し、上記202件に含めています。マーカー変更前の成功行との比較では100%・200%・125%／75%・VS16省略形の画素は一致し、101%の一部キーキャップでは端数丸めによる1pxの配置差があります。

証跡は `keycap-svg-fixed-release.avi`、`keycap-svg-fixed-release-frame.png`、`keycap-svg-fixed-release.framemd5`。1920×1080 / 30fps / 1秒の全30フレームの画素MD5は `482d03929affaa6a8290d124df095a67` で一致しました。修正前の比較行の崩れを含む出力は `keycap-svg-release-initial.avi` として残しています。下記のAVI結果は以前の版の履歴です。

## 2026-10-25: 下位フォントのキーキャップをまとめて描画

前の `beta-emoji-keycap-fix` はキーキャップを下位の数字範囲へ渡した後、SVGを持たないフォントを標準描画へ戻していたため、結合記号の分離・欠落が残っていました。キーキャップではSVG判定を必須とせず、選択後のフォントで書記素全体をDirectWrite描画し、国旗と同じ配置用スロットへ合成するよう修正しました。COLR/CPALと単色の字形も対象です。単色の結合記号が数字の字送りからはみ出す場合は、描画画像全体をスロット幅へ収め、隣接キーキャップとの重なりを避けます。

全機能テスト199件成功・3件ignore。Segoe UI Emojiで数字・`#`・`*`の12種類×VS16あり／なし×水平100%／200%をカラー描画し、字送りが分裂していないことを確認しました。Segoe UI EmojiとYu Gothic UIを使い、単独の数字をそのまま残すこと、反復キーキャップの順序・字形画像・配置幅も確認しました。Clippy（全ターゲット・全機能、`-D warnings`）、fmt、diff checkは成功しています。

専用AviUtl2 v2.1.14の `keycap-proof.aup2` を最終release DLLで再起動し、映像プレビューとRGB24 AVIで `1️⃣`・`2⃣`・`#️⃣`・`*️⃣` を確認しました。Twitter Color Emojiから下位の数字範囲／数字分類へ渡した数字キーキャップは青い合成字形となり、Segoeの絵文字行では4種すべてが水平200%の青い字形になりました。記号分類にフォント未指定の行では `#️⃣`・`*️⃣` は単色ですが、枠同士の重なりはなくなっています。単独の数字は水平100%を維持し、Twitterの国旗・ZWJのSVG合成も維持しました。キーキャップ以外のCOLR絵文字は従来どおり標準描画に従い、今回の画像変換の対象ではありません。

証跡は `target/comfont-scale-verify-20261025/keycap-render-fixed-release.avi`、`keycap-render-fixed-release-frame.png`、`keycap-render-fixed-release.framemd5` です。1920×1080 / 30fps / 1秒の全30フレームの画素MD5は `f710215b334a032d07c1c59ef7c9555e` で一致しました。配布物は `release/vandojapan.comfont-v0.3.3-beta-emoji-keycap-render-fix.au2pkg.zip`（SHA-256 `145E9D5652B86426938C01270193E74B9E9F0AE45EE3E1B3AC6C43357F9BFD40`）です。同梱・実機配置・release出力DLLのSHA-256は `8250A69AE5B9906AE3C612B88C0D48C9D7F4E8398DE316F3E1B3640DD0DD2FBA` で一致します。

## 2026-10-25: 単独の数字の除外とキーキャップの下位ルールへの引き渡し（旧版）

以下は `beta-emoji-keycap-fix` の記録です。残っていたキーキャップの崩れは、上記の描画修正版で対応しました。

絵文字プリセットの公式対象集合を保持したまま、数字・`#`・`*` にはキーキャップの書記素として一致したときだけ絵文字行を適用するよう変更しました。VS16のある／ないキーキャップを分割せず、絵文字フォントの候補が先頭文字または結合キーキャップを持たない場合は、下位の一致するカスタム範囲、次に数字・記号の分類へ書記素全体を渡します。下位へ渡す際は絵文字行の倍率やサイズも外します。通常の単一候補では不要なフォント読み込みを避ける既存の動作を維持します。

全機能テスト198件成功・3件ignore。数字12種類（`0`～`9`・`#`・`*`）、キーキャップの2形式、構成文字の欠落、絵文字行内の候補フォント、下位の範囲／分類、分類の無効化、本文フォントタグ、保存後・名前変更後のプリセットを検証しました。公式emoji-testの全シーケンスも範囲判定を通ることを確認しました。Clippy（全ターゲット・全機能、`-D warnings`）、fmt、diff checkは成功しています。

専用AviUtl2 v2.1.14環境 `target/comfont-scale-verify-20261025` にrelease DLLを配置し、`keycap-proof.aup2` でTwitter Color Emoji／Segoe UI Emojiの絵文字行を水平200%に設定しました。通常の数字・`#`・`*` は拡大されず、Twitterの未対応キーキャップは下位の数字範囲または数字分類へ渡り、絵文字行のない比較行と同じ大きさになることを映像プレビュー・RGB24 AVIで確認しました。Twitterの日本・米国の国旗とZWJ絵文字のカラー合成も維持しています。

この確認は優先度と倍率の引き渡しの検証です。AviUtl2の標準描画では、下位に指定したSegoe UI Emojiのキーキャップに欠落した結合記号や重なりが残りました。Segoeの絵文字行を使う場合にも同様の制約があり、カラー合成字形として正しく描けることは保証しません。

証跡は `keycap-release.avi`、`keycap-release-frame.png`、`keycap-release.framemd5` です。1920×1080 / 30fps / 1秒の全30フレームの画素MD5は `36d10ac071f738dc163dfe039d397b5b` で一致しました。修正版は `release/vandojapan.comfont-v0.3.3-beta-emoji-keycap-fix.au2pkg.zip`（SHA-256 `5E6F67D6CDFCFE3EC611F2C06E802B945485FFDD4D8BF9387FE7D1A521803265`）です。同梱DLL・実機配置DLL・release出力DLLのSHA-256は `A3066BBF0A943CEF6692E17A14222232AEE64D19832990F33B2C0DCF35FE49A7` で一致します。以前の水平倍率修正も含みます。

## 2026-10-25: 水平100%超でSVG合成が解除される問題の修正

水平125%で `Native text pixels differ between marker and blank passes` を再現しました。AviUtl2の `<tw>` は字形を変形しても字送りを変えないため、1em幅のマーカーを拡大すると後続文字へ重なり、画素照合に失敗していました。

補助フォントをv2へ更新し、幅1・2・4・8・16・32emのマーカー／透明文字を収録しました。マーカーの横倍率を100%以下に保ち、字間で必要な字送りへ合わせます。その際に生じる半字間の原点差もSVG配置へ補正します。通常の倍率ではフォントサイズを高さに合わせたままにし、行高の不要な増加を避けます。

専用AviUtl2 v2.1.14環境 `target/comfont-scale-verify-20261025` で、32pxのTwitter Color Emojiを水平100%・101%・125%・150%・200%（垂直100%）、水平125%／垂直75%の6条件で比較しました。日本・米国・英国の国旗と反復、ZWJ・肌色を日本語に挟み、6条件すべてのカラー表示と直前文字との位置合わせを確認しました。元の6行混在本文も水平125%でdebug版の合成成功を確認しています。

最終release DLLを配置・再起動してRGB24 AVIへ1920×1080 / 30fps / 1秒を書き出し、抽出フレームでも6条件の国旗・カラー・倍率・位置を確認しました。証跡は `scale-fixed-release.avi`、`scale-fixed-release-frame.png`、`scale-fixed-release.framemd5` です。全30フレームの画素MD5は `639d2027f1dd455589e3ce681b8d5cb5` で一致しました。

修正版は `release/vandojapan.comfont-v0.3.3-beta-emoji-scale-fix.au2pkg.zip` です。ZIPのSHA-256は `4C9F48E2183F4F2AB02C7FAB8007095C60C1C4BD040502C50BE02891CF2A7A34`、同梱・実機配置・release出力DLLは `BC08387881541B95F8930B7CAD7D6A2715EEC796EBCB02D87C6CE4493B4E2769` で一致します。全機能テスト193件成功・3件ignore、Twitter専用テスト1件成功、Clippy・fmt・diff checkも成功しました。

## 2026-10-25: 0.3.3 βのreleaseビルドとパッケージ

`cargo build --workspace --release --offline --locked` で、編集画面が参照する `UnicodeRange.subranges`、`UnicodePreset.range_label` / `to_range` が存在しないという12件のエラーを再現しました。ソースには定義が存在しますが、Cargoが変更前のcoreとmoduleのrelease生成物をFreshとして再利用していました。ワークスペース内4クレートのrelease生成物を `cargo clean -p … --release` で削除して再生成し、ビルド成功を確認しました。依存ライブラリ全体や検証用ホストの削除は不要でした。

Cargo・ロックファイル・AviUtl2設定・READMEを0.3.3へ更新しました。`scripts/package-beta.ps1` はCargo metadataから版番号を取得し、releaseビルド成功後に梱包するよう変更しました。生成した `release/vandojapan.comfont-v0.3.3-beta.au2pkg.zip` は8ファイルで、同梱DLLとreleaseビルドのSHA-256一致、package.iniの `comfont v0.3.3 β` を確認しました。

release構成の全機能テストは191件成功・3件ignore、fmtとdiff checkも成功しました。ZIPのSHA-256は `F4906F4EF2DF5283CDEDE6BAEC07B403887EB024BB1B3EFE17D3ADB4BA12313C`、DLLは `A10461CAAEA678E1F9CDEB3B9D440D9E1DF78C8BBDA3E93B22AEE691B1EE1F4C` です。

## 2026-10-25: 既存comfont内でのSVG絵文字合成

既存の「描画@comfont」で、SVGフォントを指定した絵文字だけを抽出してラスタライズする経路を追加しました。出現ごとの識別色を持つ内部スロットで位置を取得し、同じ字送りの透明スロットで描いた通常文字へ合成します。追加フィルタ・別aux2・本文編集は不要です。

実機調査では、SVGの国旗画像とRegional Indicatorペアのシェーピングは正しく、マーカーと透明スロットの画像幅が1px異なるために標準描画へ戻っていました。通常文字の画素一致から原点を求める処理へ変更しました。SDKが更新した画像中心をaviutl2-rsの保持値から取得できない問題も回避しています。また、標準テキストのサイズと `<sN>` をpxとして扱い、絵文字以降の通常文字が半分になる問題を修正しました。

専用AviUtl2 v2.1.14環境 `target/comfont-emoji-bypass-verify-20261025` で、Twitter Color Emojiを指定して日本・米国・英国の国旗、ZWJ、肌色、ユーザー提示の6行の文章を確認しました。ログの `ordered SVG emoji slots composited with native text` と、明示的に有効にしたdebugログの `bypass uploaded successfully` で、標準描画へのフォールバックではなくSVG合成の成功を確認しています。

RGB24 AVIへ1920×1080 / 30fps / 1秒を書き出し、抽出したフレームでも3か国の旗と混在文章を確認しました。出力は `emoji-flags-hybrid-proof.avi`、抽出画像は `emoji-flags-hybrid-frame.png` です。全30フレームの画素ハッシュが一致しました。当時はcmap経由でTwitterのキーキャップSVGを取得できず、対象外でした。後の調査でSVG自体の収録を確認し、上記のSVG修正版で対応しました。

最終debugビルド（SHA-256 `EDB350109C42F6DE7BE4DB6DFB49924732FDB992EE365FAA362AC52908C9D844`）へ更新・再起動し、`emoji-flags-final.avi` と `emoji-flags-final-frame.png` でも再確認しました。全30フレームの画素MD5は `e9b3ace5a3a5a52ff17de6e8942adbbc` で、修正直後の出力とも一致します。

ワークスペースの全機能テスト191件成功・3件ignore、Twitter専用テストを明示実行して1件成功。順序付きの反復国旗・ZWJ・肌色・6行のサンプル、スロットの識別、通常文字の画素保持、1pxの切り出し差、原点移動、サイズ単位を検証しました。Clippy（全ターゲット・全機能、`-D warnings`）、fmt、diff checkも成功しました。対象条件と上限は[絵文字の設定と制約](emoji.md)を参照してください。
