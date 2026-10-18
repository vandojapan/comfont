# Native standard text experiment

製品・配布パッケージから独立した、aviutl2-rs 0.50.0 / AviUtl2 v2.1.14用の検証プラグインです。

```powershell
cargo build --manifest-path tools/native-text-probe/Cargo.toml --offline --locked
```

DLLを `target/comfont-native-text-verify-20261018/data/Plugin/comfont-native-text-probe.aux2`、同梱anm2を同ホストの `data/Script` に配置し、そのAviUtl2を起動します。別のホスト名では検証処理を実行しません。空の検証プロジェクトに標準テキストを2つ作成し、以下を比較します。

- Layer1: 制御タグを本文に直接書いた標準テキストの参照。
- Layer2: 本文「原文 ABC ffi」を保持し、anm2のプロファイル指定（probe）と処理委譲から、aux2内のフィルタが `exec_effect("テキスト", …)` で同じ一時文字列を標準描画するテキスト。

フィルタ内の画素取得は比較専用で、画素を書き戻す処理はありません。ログと検証プロジェクトはホストのフォルダーへ保存します。これはフォント・サイズ・色を固定したAPIの実証であり、製品プロファイルとの連携実装ではありません。
