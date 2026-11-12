import type { Language } from './translations';

// Why a song failed, in the window's language. The service sends a code and fields beside
// its English text (crates/music-server/src/song_errors.rs); here is a template for each
// code. The service's test `every_code_has_a_translation_in_every_language` checks that each
// code has a line in all five languages. The codes are the same in every music studio; ACE-Step
// adds its own engine's (lost_kv_short … no_length).

type ErrorTexts = Record<string, string>;

const EN: ErrorTexts = {
  training_on_card: 'A LoRA is training on the graphics card; make songs once it finishes.',
  lost_vram: 'The graphics card ran out of memory on this song and the engine restarted, so the song was lost. Choose a smaller model set in Settings → Models or a shorter song, and close whatever else uses the card; a card below the smallest set cannot make songs.',
  lost_ram: 'The computer ran out of memory (RAM) on this song and the engine restarted, so the song was lost. Close other programs that take memory, or choose a smaller model set in Settings → Models or a shorter song.',
  lost_device: 'The graphics card failed during this song ({detail}) and the engine restarted, so the song was lost. The engine log is in Settings → Local engine.',
  lost_device_silent: 'The graphics card failed during this song (a CUDA or driver error) and the engine restarted, so the song was lost. The engine log is in Settings → Local engine.',
  lost_at_line: 'The engine stopped during this song and restarted, so the song was lost; its last line was {line}. The engine log is in Settings → Local engine.',
  lost: 'The engine stopped during this song and restarted, so the song was lost. The engine log is in Settings → Local engine.',
  lost_exited: 'The engine crashed during this song (exit code {exit}) and restarted, so the song was lost. The engine log is in Settings → Local engine.',
  lost_port_set: 'The engine could not listen on port {port}, which STUDIO_ENGINE_PORT sets, so the song was lost: another program holds it. Close that program or set another port.',
  lost_port_taken: 'The engine could not listen on port {port}, so the song was lost: another program took the port after the studio chose it. Restart the studio: it chooses a free port at start.',
  lost_illegal_instruction: 'The engine stopped during this song on an instruction this processor does not have (exit code {exit}) and restarted, so the song was lost. Please report it with the engine log from Settings → Local engine.',
  engine_fatal: 'The engine stopped this song: {line}',
  engine_failed: 'The engine stopped this song and its log names no cause. The log is in Settings → Local engine.',
  adapter_missing: 'The LoRA {name} is not installed; add it again on the LoRA page.',
  lost_kv_short: 'The language model ran out of room on this song: its KV cache (Settings → Local engine → LM KV cache) is smaller than the song needs. Leave that field empty to use the model\'s own size, or set it higher.',
  out_of_memory: 'There was not enough memory for this song ({detail}). Choose a smaller model set in Settings → Models or a shorter song, and close whatever else uses the card or the computer\'s memory.',
  dit_not_finite: 'The DiT wrote values that are not numbers ({count} of {total}), so the song was not made. Make it again with another seed; if it repeats, try another DiT or VAE in Settings → Models.',
  vae_not_finite: 'The VAE decoded values that are not numbers ({count} of {total}), so the song was not made. If you use a VAE from outside the studio\'s sets, try the default one in Settings → Models.',
  adapter_misfit: 'The LoRA {name} does not fit the chosen DiT: none of its layers matched the model, so the song was not made. Switch the DiT above the create form to the model the LoRA was trained for, or leave the LoRA out.',
  adapter_wrong_size: '{name} is a LoRA for {made_for} models and the chosen DiT is {dit}: switch the DiT above the create form to {made_for} or leave this LoRA out.',
  output_silence: 'The engine returned silence instead of a song, so nothing was kept. Make it again; if it repeats, the engine log has the cause.',
  output_not_numbers: 'The engine returned samples that are not numbers instead of a song, so nothing was kept. Make it again; if it repeats, the engine log has the cause.',
  output_flat: 'The engine returned one level held from start to end instead of a song, so nothing was kept. Make it again; if it repeats, the engine log has the cause.',
  track_too_long: 'This track is {length} long, and the engine takes a source or reference of under 10 minutes. Use a part of it shorter than that.',
  source_missing: 'The source track {id} is no longer in the library; choose another track.',
  reference_missing: 'The reference track {id} is no longer in the library; choose another track.',
  model_missing: '{name} is no longer on disk; choose another in Settings → Models.',
  source_required: '{task} works on a recording: choose the source track.',
  no_length: 'The language model gave the song no length: set the duration in the form.',
};

const RU: ErrorTexts = {
  training_on_card: 'На видеокарте идёт обучение LoRA: песни можно делать, когда оно закончится.',
  lost_vram: 'На этой песне у видеокарты кончилась память, движок перезапустился, и песня потеряна. Выберите набор моделей поменьше в Настройках → Модели или песню короче и закройте всё, что ещё занимает видеокарту; на карте слабее самого маленького набора песни не делаются.',
  lost_ram: 'На этой песне у компьютера кончилась оперативная память (RAM), движок перезапустился, и песня потеряна. Закройте программы, которые занимают память, или выберите набор моделей поменьше в Настройках → Модели или песню короче.',
  lost_device: 'Во время этой песни видеокарта дала сбой ({detail}), движок перезапустился, и песня потеряна. Лог движка — в Настройках → Локальный движок.',
  lost_device_silent: 'Во время этой песни видеокарта дала сбой (ошибка CUDA или драйвера), движок перезапустился, и песня потеряна. Лог движка — в Настройках → Локальный движок.',
  lost_at_line: 'Движок остановился во время этой песни и перезапустился, песня потеряна; его последняя строка: {line}. Лог движка — в Настройках → Локальный движок.',
  lost: 'Движок остановился во время этой песни и перезапустился, песня потеряна. Лог движка — в Настройках → Локальный движок.',
  lost_exited: 'Движок упал во время этой песни (код выхода {exit}) и перезапустился, песня потеряна. Лог движка — в Настройках → Локальный движок.',
  lost_port_set: 'Движок не смог слушать порт {port}, заданный в STUDIO_ENGINE_PORT, песня потеряна: порт занят другой программой. Закройте её или задайте другой порт.',
  lost_port_taken: 'Движок не смог слушать порт {port}, песня потеряна: порт заняла другая программа после того, как студия его выбрала. Перезапустите студию — при старте она выберет свободный порт.',
  lost_illegal_instruction: 'Движок остановился во время этой песни на инструкции, которой нет у этого процессора (код выхода {exit}), и перезапустился, песня потеряна. Сообщите об этом и приложите лог движка из Настроек → Локальный движок.',
  engine_fatal: 'Движок остановил эту песню: {line}',
  engine_failed: 'Движок остановил эту песню, а в его логе нет причины. Лог — в Настройках → Локальный движок.',
  adapter_missing: 'LoRA {name} не установлена: добавьте её заново на странице LoRA.',
  lost_kv_short: 'Языковой модели не хватило места на этой песне: её KV-кеш (Настройки → Локальный движок → KV-кеш LM) меньше, чем нужно песне. Оставьте это поле пустым, чтобы взять размер самой модели, или поставьте больше.',
  out_of_memory: 'Для этой песни не хватило памяти ({detail}). Выберите набор моделей поменьше в Настройках → Модели или песню короче и закройте всё, что ещё занимает видеокарту или память компьютера.',
  dit_not_finite: 'DiT выдал значения, которые не являются числами ({count} из {total}), и песня не получилась. Сделайте её снова с другим зерном; если повторяется, попробуйте другой DiT или VAE в Настройках → Модели.',
  vae_not_finite: 'VAE раскодировал значения, которые не являются числами ({count} из {total}), и песня не получилась. Если у вас VAE не из наборов студии, попробуйте стандартный в Настройках → Модели.',
  adapter_misfit: 'LoRA {name} не подходит к выбранному DiT: ни один её слой не совпал с моделью, и песня не сделана. Переключите DiT над формой на модель, под которую обучена LoRA, или уберите LoRA.',
  adapter_wrong_size: '{name} — LoRA для моделей {made_for}, а выбран DiT {dit}: переключите DiT над формой на {made_for} или уберите эту LoRA.',
  output_silence: 'Движок вернул тишину вместо песни, и ничего не сохранено. Сделайте её снова; если повторяется, причина — в логе движка.',
  output_not_numbers: 'Движок вернул отсчёты, которые не являются числами, вместо песни, и ничего не сохранено. Сделайте её снова; если повторяется, причина — в логе движка.',
  output_flat: 'Движок вернул один уровень от начала до конца вместо песни, и ничего не сохранено. Сделайте её снова; если повторяется, причина — в логе движка.',
  track_too_long: 'Этот трек длится {length}, а движок берёт исходник или референс короче 10 минут. Возьмите его часть короче.',
  source_missing: 'Исходного трека {id} больше нет в медиатеке: выберите другой трек.',
  reference_missing: 'Референсного трека {id} больше нет в медиатеке: выберите другой трек.',
  model_missing: '{name} больше нет на диске: выберите другую модель в Настройках → Модели.',
  source_required: '{task} работает с записью: выберите исходный трек.',
  no_length: 'Языковая модель не дала песне длительность: задайте её в форме.',
};

const ZH: ErrorTexts = {
  training_on_card: '显卡正在训练 LoRA，训练结束后再生成歌曲。',
  lost_vram: '生成这首歌时显卡显存耗尽，引擎已重启，这首歌丢失了。请在“设置 → 模型”中选择更小的模型组合或更短的歌曲，并关闭其他占用显卡的程序；显卡低于最小组合时无法生成歌曲。',
  lost_ram: '生成这首歌时电脑内存（RAM）耗尽，引擎已重启，这首歌丢失了。请关闭占用内存的程序，或在“设置 → 模型”中选择更小的模型组合或更短的歌曲。',
  lost_device: '生成这首歌时显卡出错（{detail}），引擎已重启，这首歌丢失了。引擎日志在“设置 → 本地引擎”。',
  lost_device_silent: '生成这首歌时显卡出错（CUDA 或驱动错误），引擎已重启，这首歌丢失了。引擎日志在“设置 → 本地引擎”。',
  lost_at_line: '生成这首歌时引擎停止并已重启，这首歌丢失了；它的最后一行是 {line}。引擎日志在“设置 → 本地引擎”。',
  lost: '生成这首歌时引擎停止并已重启，这首歌丢失了。引擎日志在“设置 → 本地引擎”。',
  lost_exited: '生成这首歌时引擎崩溃（退出码 {exit}）并已重启，这首歌丢失了。引擎日志在“设置 → 本地引擎”。',
  lost_port_set: '引擎无法监听 STUDIO_ENGINE_PORT 指定的端口 {port}，这首歌丢失了：该端口被其他程序占用。请关闭该程序或设置其他端口。',
  lost_port_taken: '引擎无法监听端口 {port}，这首歌丢失了：工作室选定该端口后被其他程序占用。请重启工作室，它会在启动时选择空闲端口。',
  lost_illegal_instruction: '生成这首歌时引擎遇到此处理器不支持的指令而停止（退出码 {exit}）并已重启，这首歌丢失了。请附上“设置 → 本地引擎”中的引擎日志反馈此问题。',
  engine_fatal: '引擎停止了这首歌：{line}',
  engine_failed: '引擎停止了这首歌，日志中没有写明原因。日志在“设置 → 本地引擎”。',
  adapter_missing: 'LoRA {name} 未安装：请在 LoRA 页面重新添加。',
  lost_kv_short: '语言模型在这首歌上空间不足：它的 KV 缓存（“设置 → 本地引擎 → LM KV 缓存”）小于这首歌所需。将该字段留空以使用模型自身的大小，或调大它。',
  out_of_memory: '这首歌的内存不足（{detail}）。请在“设置 → 模型”中选择更小的模型组合或更短的歌曲，并关闭其他占用显卡或电脑内存的程序。',
  dit_not_finite: 'DiT 输出了非数值（{total} 个中的 {count} 个），这首歌没有生成。请换一个种子重新生成；如果再次出现，请在“设置 → 模型”中换一个 DiT 或 VAE。',
  vae_not_finite: 'VAE 解码出了非数值（{total} 个中的 {count} 个），这首歌没有生成。如果你使用的 VAE 不在工作室的组合中，请在“设置 → 模型”中改用默认 VAE。',
  adapter_misfit: 'LoRA {name} 与所选 DiT 不匹配：它没有任何一层与模型对应，这首歌没有生成。请把创建表单上方的 DiT 切换为该 LoRA 训练时使用的模型，或不使用该 LoRA。',
  adapter_wrong_size: '{name} 是用于 {made_for} 模型的 LoRA，而所选 DiT 是 {dit}：请把创建表单上方的 DiT 切换为 {made_for}，或不使用该 LoRA。',
  output_silence: '引擎返回的是静音而不是歌曲，什么也没有保留。请重新生成；如果再次出现，原因在引擎日志中。',
  output_not_numbers: '引擎返回的是非数值的采样而不是歌曲，什么也没有保留。请重新生成；如果再次出现，原因在引擎日志中。',
  output_flat: '引擎返回的是从头到尾不变的电平而不是歌曲，什么也没有保留。请重新生成；如果再次出现，原因在引擎日志中。',
  track_too_long: '这条音轨长 {length}，而引擎只接受短于 10 分钟的源音频或参考音频。请使用其中较短的一段。',
  source_missing: '源音轨 {id} 已不在媒体库中：请选择其他音轨。',
  reference_missing: '参考音轨 {id} 已不在媒体库中：请选择其他音轨。',
  model_missing: '{name} 已不在磁盘上：请在“设置 → 模型”中选择其他模型。',
  source_required: '{task} 需要一段录音：请选择源音轨。',
  no_length: '语言模型没有给出歌曲时长：请在表单中设置时长。',
};

const JA: ErrorTexts = {
  training_on_card: 'グラフィックカードで LoRA の学習中です。終わってから曲を作ってください。',
  lost_vram: 'この曲でグラフィックカードのメモリが足りなくなり、エンジンが再起動したため曲は失われました。「設定 → モデル」で小さいモデルセットか短い曲を選び、カードを使っている他のものを閉じてください。最小のセットに満たないカードでは曲を作れません。',
  lost_ram: 'この曲でパソコンのメモリ（RAM）が足りなくなり、エンジンが再起動したため曲は失われました。メモリを使う他のプログラムを閉じるか、「設定 → モデル」で小さいモデルセットか短い曲を選んでください。',
  lost_device: 'この曲の途中でグラフィックカードが失敗し（{detail}）、エンジンが再起動したため曲は失われました。エンジンのログは「設定 → ローカルエンジン」にあります。',
  lost_device_silent: 'この曲の途中でグラフィックカードが失敗し（CUDA またはドライバーのエラー）、エンジンが再起動したため曲は失われました。エンジンのログは「設定 → ローカルエンジン」にあります。',
  lost_at_line: 'この曲の途中でエンジンが止まって再起動し、曲は失われました。最後の行：{line}。エンジンのログは「設定 → ローカルエンジン」にあります。',
  lost: 'この曲の途中でエンジンが止まって再起動し、曲は失われました。エンジンのログは「設定 → ローカルエンジン」にあります。',
  lost_exited: 'この曲の途中でエンジンがクラッシュし（終了コード {exit}）、再起動したため曲は失われました。エンジンのログは「設定 → ローカルエンジン」にあります。',
  lost_port_set: 'エンジンが STUDIO_ENGINE_PORT で指定したポート {port} で待ち受けできず、曲は失われました。別のプログラムがそのポートを使っています。そのプログラムを閉じるか、別のポートを指定してください。',
  lost_port_taken: 'エンジンがポート {port} で待ち受けできず、曲は失われました。スタジオが選んだ後に別のプログラムがそのポートを使い始めました。スタジオを再起動してください。起動時に空いているポートを選びます。',
  lost_illegal_instruction: 'この曲の途中でエンジンがこのプロセッサーにない命令で止まり（終了コード {exit}）、再起動したため曲は失われました。「設定 → ローカルエンジン」のエンジンログを添えて報告してください。',
  engine_fatal: 'エンジンがこの曲を止めました：{line}',
  engine_failed: 'エンジンがこの曲を止めましたが、ログに理由が書かれていません。ログは「設定 → ローカルエンジン」にあります。',
  adapter_missing: 'LoRA {name} はインストールされていません。LoRA ページでもう一度追加してください。',
  lost_kv_short: 'この曲で言語モデルの領域が足りなくなりました。KV キャッシュ（「設定 → ローカルエンジン → LM KV キャッシュ」）が曲に必要な大きさより小さいためです。この欄を空にしてモデル自身の大きさを使うか、大きくしてください。',
  out_of_memory: 'この曲にはメモリが足りませんでした（{detail}）。「設定 → モデル」で小さいモデルセットか短い曲を選び、カードやパソコンのメモリを使っている他のものを閉じてください。',
  dit_not_finite: 'DiT が数値でない値を出力したため（{total} 個中 {count} 個）、曲は作られませんでした。別のシードでもう一度作ってください。繰り返す場合は「設定 → モデル」で別の DiT か VAE を試してください。',
  vae_not_finite: 'VAE が数値でない値をデコードしたため（{total} 個中 {count} 個）、曲は作られませんでした。スタジオのセット以外の VAE を使っている場合は、「設定 → モデル」で標準の VAE を試してください。',
  adapter_misfit: 'LoRA {name} は選んだ DiT に合いません。どの層もモデルと一致しなかったため、曲は作られませんでした。作成フォームの上の DiT を LoRA の学習に使われたモデルに切り替えるか、LoRA を外してください。',
  adapter_wrong_size: '{name} は {made_for} モデル用の LoRA ですが、選んだ DiT は {dit} です。作成フォームの上の DiT を {made_for} に切り替えるか、この LoRA を外してください。',
  output_silence: 'エンジンが曲ではなく無音を返したため、何も保存されませんでした。もう一度作ってください。繰り返す場合、原因はエンジンのログにあります。',
  output_not_numbers: 'エンジンが曲ではなく数値でないサンプルを返したため、何も保存されませんでした。もう一度作ってください。繰り返す場合、原因はエンジンのログにあります。',
  output_flat: 'エンジンが曲ではなく最初から最後まで同じレベルを返したため、何も保存されませんでした。もう一度作ってください。繰り返す場合、原因はエンジンのログにあります。',
  track_too_long: 'このトラックの長さは {length} ですが、エンジンが受け付けるソースやリファレンスは 10 分未満です。それより短い部分を使ってください。',
  source_missing: 'ソーストラック {id} はもうライブラリにありません。別のトラックを選んでください。',
  reference_missing: 'リファレンストラック {id} はもうライブラリにありません。別のトラックを選んでください。',
  model_missing: '{name} はもうディスクにありません。「設定 → モデル」で別のモデルを選んでください。',
  source_required: '{task} は録音を使います。ソーストラックを選んでください。',
  no_length: '言語モデルが曲の長さを決めませんでした。フォームで長さを指定してください。',
};

const KO: ErrorTexts = {
  training_on_card: '그래픽카드에서 LoRA 학습 중입니다. 학습이 끝난 뒤 곡을 만드세요.',
  lost_vram: '이 곡에서 그래픽카드 메모리가 부족해 엔진이 다시 시작되었고 곡을 잃었습니다. "설정 → 모델"에서 더 작은 모델 세트나 더 짧은 곡을 고르고, 카드를 쓰는 다른 것을 닫으세요. 가장 작은 세트보다 약한 카드로는 곡을 만들 수 없습니다.',
  lost_ram: '이 곡에서 컴퓨터 메모리(RAM)가 부족해 엔진이 다시 시작되었고 곡을 잃었습니다. 메모리를 쓰는 다른 프로그램을 닫거나 "설정 → 모델"에서 더 작은 모델 세트나 더 짧은 곡을 고르세요.',
  lost_device: '이 곡을 만드는 중 그래픽카드에 오류가 나서({detail}) 엔진이 다시 시작되었고 곡을 잃었습니다. 엔진 로그는 "설정 → 로컬 엔진"에 있습니다.',
  lost_device_silent: '이 곡을 만드는 중 그래픽카드에 오류가 나서(CUDA 또는 드라이버 오류) 엔진이 다시 시작되었고 곡을 잃었습니다. 엔진 로그는 "설정 → 로컬 엔진"에 있습니다.',
  lost_at_line: '이 곡을 만드는 중 엔진이 멈췄다가 다시 시작되어 곡을 잃었습니다. 마지막 줄: {line}. 엔진 로그는 "설정 → 로컬 엔진"에 있습니다.',
  lost: '이 곡을 만드는 중 엔진이 멈췄다가 다시 시작되어 곡을 잃었습니다. 엔진 로그는 "설정 → 로컬 엔진"에 있습니다.',
  lost_exited: '이 곡을 만드는 중 엔진이 비정상 종료되었고(종료 코드 {exit}) 다시 시작되어 곡을 잃었습니다. 엔진 로그는 "설정 → 로컬 엔진"에 있습니다.',
  lost_port_set: '엔진이 STUDIO_ENGINE_PORT에 지정된 포트 {port}에서 대기할 수 없어 곡을 잃었습니다. 다른 프로그램이 그 포트를 쓰고 있습니다. 그 프로그램을 닫거나 다른 포트를 지정하세요.',
  lost_port_taken: '엔진이 포트 {port}에서 대기할 수 없어 곡을 잃었습니다. 스튜디오가 포트를 고른 뒤 다른 프로그램이 그 포트를 차지했습니다. 스튜디오를 다시 시작하세요. 시작할 때 빈 포트를 고릅니다.',
  lost_illegal_instruction: '이 곡을 만드는 중 엔진이 이 프로세서에 없는 명령에서 멈췄고(종료 코드 {exit}) 다시 시작되어 곡을 잃었습니다. "설정 → 로컬 엔진"의 엔진 로그와 함께 알려 주세요.',
  engine_fatal: '엔진이 이 곡을 멈췄습니다: {line}',
  engine_failed: '엔진이 이 곡을 멈췄지만 로그에 이유가 없습니다. 로그는 "설정 → 로컬 엔진"에 있습니다.',
  adapter_missing: 'LoRA {name}이(가) 설치되어 있지 않습니다. LoRA 페이지에서 다시 추가하세요.',
  lost_kv_short: '이 곡에서 언어 모델의 공간이 모자랐습니다. KV 캐시("설정 → 로컬 엔진 → LM KV 캐시")가 곡에 필요한 크기보다 작습니다. 이 칸을 비워 모델 자체 크기를 쓰거나 더 크게 설정하세요.',
  out_of_memory: '이 곡에 쓸 메모리가 부족했습니다({detail}). "설정 → 모델"에서 더 작은 모델 세트나 더 짧은 곡을 고르고, 카드나 컴퓨터 메모리를 쓰는 다른 것을 닫으세요.',
  dit_not_finite: 'DiT가 숫자가 아닌 값을 냈기 때문에({total}개 중 {count}개) 곡을 만들지 못했습니다. 다른 시드로 다시 만드세요. 반복되면 "설정 → 모델"에서 다른 DiT나 VAE를 써 보세요.',
  vae_not_finite: 'VAE가 숫자가 아닌 값을 디코딩했기 때문에({total}개 중 {count}개) 곡을 만들지 못했습니다. 스튜디오 세트 밖의 VAE를 쓰고 있다면 "설정 → 모델"에서 기본 VAE를 써 보세요.',
  adapter_misfit: 'LoRA {name}이(가) 선택한 DiT에 맞지 않습니다. 어떤 층도 모델과 일치하지 않아 곡을 만들지 못했습니다. 만들기 양식 위의 DiT를 LoRA가 학습된 모델로 바꾸거나 LoRA를 빼세요.',
  adapter_wrong_size: '{name}은(는) {made_for} 모델용 LoRA인데 선택한 DiT는 {dit}입니다. 만들기 양식 위의 DiT를 {made_for}(으)로 바꾸거나 이 LoRA를 빼세요.',
  output_silence: '엔진이 곡 대신 무음을 돌려주어 아무것도 저장하지 않았습니다. 다시 만드세요. 반복되면 원인은 엔진 로그에 있습니다.',
  output_not_numbers: '엔진이 곡 대신 숫자가 아닌 샘플을 돌려주어 아무것도 저장하지 않았습니다. 다시 만드세요. 반복되면 원인은 엔진 로그에 있습니다.',
  output_flat: '엔진이 곡 대신 처음부터 끝까지 같은 레벨을 돌려주어 아무것도 저장하지 않았습니다. 다시 만드세요. 반복되면 원인은 엔진 로그에 있습니다.',
  track_too_long: '이 트랙의 길이는 {length}인데 엔진은 10분보다 짧은 소스나 레퍼런스만 받습니다. 그보다 짧은 부분을 쓰세요.',
  source_missing: '소스 트랙 {id}이(가) 더 이상 라이브러리에 없습니다. 다른 트랙을 고르세요.',
  reference_missing: '레퍼런스 트랙 {id}이(가) 더 이상 라이브러리에 없습니다. 다른 트랙을 고르세요.',
  model_missing: '{name}이(가) 더 이상 디스크에 없습니다. "설정 → 모델"에서 다른 모델을 고르세요.',
  source_required: '{task}은(는) 녹음으로 작업합니다. 소스 트랙을 고르세요.',
  no_length: '언어 모델이 곡 길이를 정하지 않았습니다. 양식에서 길이를 지정하세요.',
};

const BY_LANGUAGE: Record<Language, ErrorTexts> = { en: EN, ru: RU, zh: ZH, ja: JA, ko: KO };

/** A failed song as the service sent it: its English text and, when the reason has a kind, a code with fields. */
export interface SongFailure {
  message?: string | null;
  code?: string | null;
  params?: Record<string, unknown> | null;
}

/** Why a song failed, in the window's language. Without a code, the service's text as it is: it shows there is no translation. */
export function songErrorText(failure: SongFailure | null | undefined, language: Language): string {
  const raw = failure?.message ?? '';
  const template = failure?.code ? BY_LANGUAGE[language][failure.code] : undefined;
  if (!template) return raw;
  const params = failure?.params ?? {};
  return template.replace(/\{(\w+)\}/g, (match, key: string) => {
    const value = params[key];
    return value === undefined || value === null ? match : String(value);
  });
}
