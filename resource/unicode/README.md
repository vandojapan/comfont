# Unicodeブロックプリセット

Unicode 17.0.0 の公式 [Blocks.txt](https://www.unicode.org/Public/17.0.0/ucd/Blocks.txt) を使用しています。
原本をこのフォルダーに保存し、Rust定数を `crates/core/src/unicode_presets.rs` に収録しています。

346ブロックのうち、Unicodeスカラー値ではないサロゲート専用3ブロックを除く343ブロックを登録します。
漢字・ひらがな・カタカナ・Basic Latin・半角／全角の5ブロックを先頭に置き、残りは開始コードポイント順です。
よく使うブロックと主要な文字体系には日本語名を付けています。検索には公式英語名も使用できます。
既存プロファイルにはブロックの番号ではなく範囲と名前を保存しているため、一覧の変更で設定は変わりません。

[Unicode License V3](LICENSE.txt) を配布パッケージにも同梱します。
