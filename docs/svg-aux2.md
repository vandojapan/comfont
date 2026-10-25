# SVGラスタライズ用aux2（試作）

これは別aux2の試作APIの資料です。**0.3.3 βのcomfont本体によるSVG絵文字の自動合成には、このaux2やサンプルobj2は不要です。** 通常の使い方・制約は[絵文字の設定と制約](emoji.md)を参照してください。

`comfont_svg.aux2` は画像を返す小さなLuaモジュールです。aviutl2-rs 0.50.0を使用し、既存の設定プレビューとSVG描画処理を共用します。標準テキストの書き換え・位置合わせ・混在本文の自動置換は担当しません。

## ビルドと配置

```powershell
cargo build -p comfont-svg-plugin --release --offline --locked
```

`target/release/comfont_svg.dll` を `Plugin/comfont_svg.aux2` として配置します。任意で `scripts/@comfont-svg.obj2` を `Script` に配置すると、「絵文字@comfont-svg」「SVGサンプル@comfont-svg」で確認できます。AviUtl2 v2.1.14以降が必要です。現時点では通常のcomfont配布パッケージには含めません。

2026-10-25、専用AviUtl2 v2.1.14で32・96・192pxの国旗とSVGサンプルのカラー表示、およびAVI書き出しを確認しました。検証内容は[検証状況](validation.md)に記録しています。

## Lua API（バージョン1）

```lua
local svg = obj.module("comfont_svg")
-- SVG XMLから、指定ピクセル数の透明PNGを生成
local path, w, h, origin_x, baseline, advance, err =
    svg.rasterize_svg(xml, 128, 128)

-- SVG-in-OpenTypeフォントの字形から生成。国旗・ZWJ・肌色も1書記素で指定
local path, w, h, origin_x, baseline, advance, err =
    svg.rasterize_glyph("🇯🇵", "Twitter Color Emoji", 128)
if not path then error(err) end
obj.load("image", path)
```

`api_version()` は `1` を返します。画像はstraight alphaのRGBA PNGです。背景は透明で、元SVGからサイズごとに描画します。同じ要求は同じパスを返し、PNGの再生成を省略します。

`rasterize_svg` の幅・高さは整数です。元SVGの `viewBox` と `preserveAspectRatio` に従って指定キャンバスへ描画します。`viewBox` がない場合は数値の `width` / `height` を座標範囲に使います。どちらもないSVGには対応しません。固定の `width` / `height` があっても出力キャンバスに合わせてベクトルから描き直します。SVG XMLは最大1 MiB、出力は一辺8192ピクセル、総1677万画素までです。

`rasterize_glyph` のサイズは**emのピクセル数**（1～4096）です。AviUtl2のUIに表示される文字サイズと合わせる場合は呼び出し元で換算してください。画像にはフォントの行高・オーバーハングを含み、サイズ値と画像の幅・高さは一致しないことがあります。`origin_x` と `baseline` は画像左上から見た論理原点X・ベースラインY、`advance` は字送りです。本文の組版に合成する際はこれらを使います。SVG XMLの場合はそれぞれ `0`・画像の高さ・画像の幅です。

文字列の上限はUTF-8で1024バイト、1書記素です。字形選択はDirectWriteに任せ、SVGとCOLR/CPALをカラー描画します。指定フォントに字形がない場合はWindowsのフォールバックになります。特にこの環境のTwitter Color Emojiには数字 `1` がなく、キーキャップのカラー字形は保証しません。単に `J` と `P` を個別に渡すと国旗にはなりません。

描画にはホストと独立したDirect2D/WARPを使います。対応するSVG表現はWindowsのDirect2Dに従います。SVGアニメーション、ビットマップカラーフォント、COLR v1ペイントツリーは対象外です。失敗時は `path=nil`、最後の戻り値にエラー文字列を返します。

PNGはセッションごとの一時フォルダーに保持し、aux2の終了時に削除します。プロジェクトにはパスを保存せず、毎回モジュールから取得してください。上限は1024画像またはPNGと要求データの合計64 MiBです。使用中の画像を途中で消さないため、上限に達した場合はエラーを返します。フォントを更新した場合はAviUtl2を再起動してください。
