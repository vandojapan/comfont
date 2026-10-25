# 設定画面の描画バックエンド

設定画面は引き続き `eframe` / `winit` のネイティブ統合を使い、egui UIとAviUtl2のメニュー・ウィンドウ統合を維持します。変更対象は設定画面の描画バックエンドです。`.aux2`、プロファイル形式、DirectWriteによる文字描画には変更を加えません。

## Windowsでの選択順

設定画面を開くと、次の順で初期化します。

1. `egui-wgpu` + Direct3D 12
2. `egui-wgpu` + Vulkan
3. `egui_glow` + OpenGL

Direct3D 12とVulkanはWindows向けwgpu機能として明示的にビルドします。DX12を最初に試し、対応GPU・ドライバーが使えないときはVulkanへ進みます。両方の初期化に失敗した場合は、従来のOpenGL経路を互換用として試します。設定画面の負荷に合わせて低消費電力アダプターを希望するため、内蔵GPUと専用GPUがある環境では内蔵GPUが選択される場合があります。wgpuデバイスにはdownlevel limitsを要求し、テクスチャ寸法は選択されたアダプターの上限に合わせます。

失敗を検出して次へ進むのは、eguiアプリが起動する前の初期化エラーです。egui UIの起動後に描画エラーが返った場合は再度別のレンダラーを起動せず、エラーをAviUtl2ログへ記録して設定画面を終了します。panicもプラグインのメニューコールバック内で捕捉してエラーにし、イベントループの状態が不明なまま別方式を重ねて起動しません。描画リソースとwinitループのライフサイクルはeframeに任せ、ホストウィンドウ監視スレッドは設定画面の終了時に停止・joinします。

## 対応範囲と制限

AviUtl2 v2.1.14以降を対象とします。AviUtl2の現行公式要件はWindows 10 64-bit以降、AVX2対応CPU、DirectX 11.3以上、ROV対応GPUです。そのため、設定画面側で別の描画APIへフォールバックしても、AviUtl2本体が要求するGPU要件を満たさない環境を利用可能にはできません。[AviUtl2公式動作環境](https://docs.aviutl2.jp/)

wgpuのWindows向け候補としてDirect3D 12とVulkanを有効にしています。wgpuはWindowsでDX12とVulkanを提供します。[wgpuのバックエンド機能](https://github.com/gfx-rs/wgpu/blob/trunk/wgpu/Cargo.toml) OpenGL経路は、wgpuの初期化が成立しない環境向けに従来の `egui_glow` を残しています。Microsoft WARPを強制選択する実装や、独自のCPUラスタライザーは含めません。ソフトウェアアダプターの利用可否はシステムのドライバーとAPIに依存します。

Windows 10/11上のNVIDIA、AMD、Intel GPUでの実機確認は、この実装環境では行っていません。RDP、古いドライバー、WARP、古いOpenGLドライバーも未確認です。リリース前の実機確認では、少なくとも次を確認してください。

| 環境 | 確認項目 |
|---|---|
| Windows 11 + NVIDIA / AMD / Intel GPU | DX12で起動、UI操作、フォントプレビュー、リサイズ、DPI変更、終了後の再オープン |
| Windows 10 + AviUtl2対応GPU | DX12とVulkanの起動、設定保存、AviUtl2の終了時に設定画面も閉じること |
| DX12アダプターを使えない環境 | Vulkanへの切替とUI描画 |
| WGPU初期化に失敗する環境 | OpenGLへの切替、すべて失敗した際のログとAviUtl2の継続動作 |
| RDP / 古いGPU・ドライバー | 初期化結果と復帰後の描画。現状は保証対象外 |

ウィンドウのリサイズ・DPI変更は既存のeframe/winit経路とwgpuサーフェス再構成に任せます。サーフェス喪失時は再作成を試し、GPUデバイス自体の喪失後に別APIへ切り替える機能はありません。
