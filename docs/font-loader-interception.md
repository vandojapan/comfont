# フォントローダー割り込み方針

## 結論

現行のAviUtl2 SDK 0.40.0には、文字列またはコードポイントごとのフォント解決へ割り込む公開コールバックはない。
そのため、フォントローダーを直接フックする構成は採用せず、テキストをDirectWriteへ渡す前にAviUtl2の制御文字へ展開する方式を第一候補とする。

## SDKで利用できる境界

- `EDIT_HANDLE.get_font(name)`は、登録済みフォント名から`IDWriteFont`を取得する参照APIであり、解決処理を置換するAPIではない。
- `HOST_APP_TABLE.register_font_collection(collection)`は`IDWriteFontCollection`を追加登録できる。ただし、文字ごとのフォント選択コールバックは受け取れない。
- Rustの`aviutl2` 0.40.0ではフォント名列挙と`get_font`はラップされているが、`register_font_collection`の安全な高水準ラッパーは公開されていない。

DirectWriteのカスタムコレクションローダーは、コレクションを構築するときにフォントファイルを列挙する仕組みである。描画中にコードポイントごとの振り分けを行う仕組みではない。
`IDWriteFontFallback`ならUnicode範囲とフォントファミリーを対応付けられるが、AviUtl2が生成した`IDWriteTextFormat1`へ`SetFontFallback`する境界がSDKにない。

## 推奨構成

### FontManager連携

`register_font_collection()`による後付け登録はFontManagerの列挙には反映されたが、
AviUtl2標準テキストのフォントドロップダウンには反映されなかった。実機ログでは、後付け登録が
共通プラグイン読込後に行われる一方、アプリケーションデータの`Font`に置いたフォントは
共通プラグイン読込前に登録された。標準UIは独自描画であり、SDKには一覧の再構築通知もない。

このため`register_font_collection()`による後付け登録は採用せず、AviUtl2が標準対応している
起動時の`Font`読込を利用する。エディターのフォントプルダウンはGDI列挙ではなく、
`EDIT_HANDLE.enum_font_name()`が返すFontManagerの登録名を、開くたびに再列挙する。
合成フォントの切り替え本体は以下の`decorate`方式を維持する。

実機で、保存済みプロファイル`aa`が`profiles.json`に存在する状態を再現して確認したところ、
FontManagerの列挙結果には含まれず、`get_font("aa")`も失敗した。これは一覧更新の問題ではなく、
プロファイルが`IDWriteFont`ではないためである。`保存`はルールをJSONへ保存する操作であり、
プロファイル名を標準フォント一覧へ登録する操作ではない。

追加フォントはアプリケーションデータの`compositefont/fonts`へ配置する。通常構成では
`C:\ProgramData\aviutl2\compositefont\fonts`、ポータブル構成ではAviUtl2本体横の
`data\compositefont\fonts`になる。エディターの`保存`ボタンを押すと
ディレクトリを再走査し、新しいファイルだけをアプリケーションデータの
`Font/compositefont`へコピーする。同名ファイルは上書きせず、追加登録だけを扱う。
コピー後にAviUtl2を再起動すると、標準UI構築前にFontManagerへ登録される。

module-pluginは次の関数を公開する。

```lua
local _, size, _, _, _, _, _, spacing = obj.getfont()
local decorated = compositefont.decorate(text, "subtitle", size, spacing)
obj.load("text", decorated)
```

`decorate`は連続する同一設定の文字をrunへまとめ、run境界へ次のAviUtl2制御文字を挿入する。

- `<@フォント名>`: フォント
- `<s*倍率>`: サイズ
- `<gw値>`: 字送り
- `<tw倍率>`: 水平比率
- `<th倍率>`: 垂直比率
- `<p+X,+Y>`: ベースライン補正が必要な場合の相対座標

run末尾では変更した設定だけを逆順にリセットする。字送りは
`既存字間 + base_font_size * tracking_adjust_em`、ベースラインのY座標は
`-base_font_size * baseline_shift_em`へ換算する。

最初の実装は制御文字を含まないプレーンテキストだけを展開する。ASCIIの`<`を含む入力は、
既存タグやPSDToolKit2の字幕を壊さないよう、エラーにせず入力をそのまま返す。不明なプロファイル、
不正な補正値、タグ区切りと衝突するフォント名も同じくfail-openする。既存制御文字との状態合成は、
AviUtl2のタグをトークン化できる段階で追加する。

PSDToolKit2の`字幕表示`では、`require("PSDToolKit").mes(o, obj)`へ渡す`o`に次を追加する。

```lua
modifier = function(text)
    local compositefont = obj.module("compositefont")
    local _, size, _, _, _, _, _, spacing = obj.getfont()
    return compositefont.decorate(text, "subtitle", size, spacing)
end,
```

初期実装は1回のmodule呼び出し内でrunを生成し、文字ごとのmodule呼び出しと`multiobject`を避ける。
キャッシュは必要性を実測してから追加し、追加する場合のキーは
`(profile revision, text, base font size, base char spacing)`とする。

## 採用しない案

### DirectWriteのインラインフック

`CreateTextLayout`や`IDWriteTextLayout::SetFontFamilyName`をDetours等で置換すれば技術上は介入できるが、AviUtl2の更新、COM vtable、描画スレッド、レイアウトキャッシュへ強く依存する。クラッシュ時にホスト全体を巻き込むため、通常配布には使わない。

### 仮想OpenTypeフォントの生成

複数フォントのcmap、グリフ、メトリクス、GSUB/GPOS、カラーフォント、可変軸を1ファイルへ再構築すれば単一ファミリーとして登録できる。しかし変形値や字送りは表現できず、フォントのライセンス問題もあるため対象外とする。

## SDKへ提案するなら

フォントコレクション登録より、テキストレイアウト直前に文字runを返すAPIが必要になる。

```text
resolve_text_runs(text, base_format) -> [
  { range, font_family, size_ratio, baseline, tracking, vertical, horizontal }
]
```

この境界が追加されれば、AviUtl2側のDirectWriteレイアウト、キャッシュ、縦書き、装飾を維持したまま合成フォントを適用できる。
