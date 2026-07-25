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

module-pluginへ次の関数を追加する。

```lua
local decorated = compositefont.decorate(text, "subtitle")
obj.load("text", decorated)
```

`decorate`は連続する同一設定の文字をrunへまとめ、run境界へ次のAviUtl2制御文字を挿入する。

- `<@フォント名>`: フォント
- `<s*倍率>`: サイズ
- `<gw値>`: 字送り
- `<tw倍率>`: 水平比率
- `<th倍率>`: 垂直比率
- `<p+X,+Y>`: ベースライン補正が必要な場合の相対座標

run末尾では各設定をリセットする。最初の実装は制御文字を含まないプレーンテキストだけを受け付け、既存の制御文字を含むテキストはエラーにする。制御文字との合成は、AviUtl2のタグをトークン化できる段階で追加する。

キャッシュキーは`(profile revision, text, base font size)`とし、プロファイル更新時に破棄する。これにより文字ごとのmodule呼び出しと`multiobject`を避けられる。

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
