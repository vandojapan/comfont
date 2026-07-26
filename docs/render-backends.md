# テキスト適用バックエンド

## 目的

現在のAviUtl2 SDKには文字runをレイアウト直前に差し替える公開APIがないため、実装は
`compositefont.decorate`で制御文字を生成する。将来、`font-loader-interception.md`で提案している
`resolve_text_runs(text, base_format)`相当のAPIが追加された場合に、プロファイル形式や文字分類を
変更せず適用部分だけを交換できる構造にする。

```text
profiles.json + text + FontManagerのグリフ情報
                    |
                    v
             resolve_text_runs
                    |
        ResolvedTextRun[]（共通表現）
                    |
          +---------+----------+
          |                    |
          v                    v
 ControlTagBackend       NativeSdkBackend
 （現在のdecorate）       （将来追加するadapter）
```

## 共通run表現

`ResolvedTextRun`は次の情報を持つ。

- 元テキスト上のUTF-8バイト範囲
- DirectWriteやWindows APIへ渡すためのUTF-16コード単位範囲
- フォントファミリー、サイズ、ベースライン、字送り、垂直比率、水平比率を含む`FontAdjustment`
- 改行runでは補正値を持たず、元の改行をそのまま維持する

run解決は文字分類、優先フォント、FontManagerのグリフ有無を扱う。AviUtl2制御文字の構文は
扱わない。隣接する同一の補正値は1つのrunへまとめる。

## 現在のバックエンド

`ControlTagBackend`だけを有効にする。`decorate`は共通runを受け取り、`<@>`, `<s>`, `<gw>`,
`<tw>`, `<th>`, `<p>`へ変換する。このバックエンドで表現できないフォント名や既存タグを検出した
場合は、従来どおり入力を変更せず返す。

## SDK追加後の移行

SDK側に文字run解決コールバックが追加されたら、次の順序で対応する。

1. SDKのrange単位を確認し、`ResolvedTextRun`のUTF-8またはUTF-16範囲へ対応させる。
2. `NativeSdkBackend`を追加し、各runの`FontAdjustment`をSDK構造体へ変換する。
3. SDKが要求するプロファイル選択方法とコールバック登録処理だけをintegration層に実装する。
4. `decorate`は既存プロジェクトとPSDToolKit2用の互換APIとして残す。
5. ネイティブ経路の利用中は制御文字を生成せず、元テキストとrun配列をホストへ渡す。

文字分類、フォールバック順、`profiles.json`、editor-pluginの保存形式は変更しない。SDK固有型を
coreやプロファイル型へ持ち込まない。

## 未確定事項

提案APIには、標準テキストオブジェクトがどの合成フォントプロファイルを選ぶかという関連付けも
必要になる。この部分はSDK仕様が確定するまで仮定せず、将来のadapter側の責務とする。
