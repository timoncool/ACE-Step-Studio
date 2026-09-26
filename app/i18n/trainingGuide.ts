import type { Language } from './translations';

/**
 * The training guide of ACE-Step Studio, per language. It follows the studio's
 * own training pipeline: HOT-Step's ace-train teaching an adapter to the DiT.
 */

export interface GuideSection {
  title: string;
  text?: string[];
  steps?: string[];
  list?: string[];
  checklist?: string[];
  examples?: { label?: string; body: string }[];
}

export interface Guide {
  title: string;
  intro?: string;
  expandAll: string;
  collapseAll: string;
  close: string;
  resize: string;
  sections: GuideSection[];
}

const CAPTION_EXAMPLE = `An energetic pop-rock track driven by a clean, arpeggiated electric guitar riff and a punchy, straightforward drum beat. A powerful female lead vocal carries the verses, the chorus opens up with layered harmonies, and a warm bass keeps the groove tight under a bright, polished mix.`;

const GENRE_EXAMPLE = `pop rock, female vocals, electric guitar, energetic, anthemic`;

const en: Guide = {
  title: 'LoRA training guide',
  intro: 'A LoRA is a small add-on to the model that learns to sound like your songs: the timbre, the vocals, the arrangements, the production. In ACE-Step Studio the DiT is what learns — the part of the model that renders the sound. Drag this window by its title and resize it from the bottom-right corner.',
  expandAll: 'Expand all',
  collapseAll: 'Collapse all',
  close: 'Close',
  resize: 'Drag to resize',
  sections: [
    {
      title: 'Workflow',
      steps: [
        'Drop a folder of songs by one artist, best of one album, onto the Training page, or pick a folder or files with the buttons. The studio makes a dataset named after the folder. WAV, MP3, FLAC, OGG and M4A work; an album in one file with a .cue is cut into its songs, lyrics in a .txt or .lrc beside a song are taken as they are.',
        'Step 1 · Songs: preparation starts by itself. The studio takes the lyrics from the databases, separates the vocals and recognises only the songs they do not know, and — with the auto-describe pack — listens to every song and writes its caption and genre tags with the measured tempo and key. Every song shows its own status, the overall progress is on top.',
        'Every song keeps where it stopped. If the studio is closed or crashes during preparation, the work carries on by itself from the same place at the next start, and nothing done is done again. A song that failed has a Retry button, an unfinished one Finish this song: they finish just that song.',
        'Check the result: click a song and it opens with a player, its caption and its lyrics. Fix what needs fixing; Describe again and Find again redo just that song.',
        'Step 2 · Training: the LoRA name, a trigger word (the studio makes a rare word from the dataset name; change or clear it as you like), the readiness check and the Train button. The trainer (about 0.1 GB) and the unquantised BF16 weights of the DiT you render with (4.5 GB for the standard turbo model) download right there, once.',
        'No need to wait: while preparation runs, tick "Start training by itself when every song is ready".',
        'Step 3 · Result: the run leaves one adapter at its end — the best one when it stopped at the target loss. Listen to it and press To LoRA; it appears on the LoRA page. While training runs, generation, the assistant, karaoke and stem separation are unavailable.',
      ],
    },
    {
      title: 'Which model it trains',
      list: [
        'The DiT the studio renders with now: its BF16 weights, because the trainer does not train quantised ones. Change the model with the switcher above the create form before you train.',
        'The adapter fits every quantisation of that model — train on BF16, generate with Q4 or Q8.',
        'It fits only that size of model: an adapter of the standard 2B DiT does not merge into an XL one, nor the other way round. The LoRA page marks which size each adapter is for.',
        'A run trained further keeps its model: switch back to it to continue.',
      ],
    },
    {
      title: 'Which songs to use',
      list: [
        'One artist, ideally one album or one era: likeness matters more than range.',
        'Usually 5 to 20 songs, whole, with their start and end.',
        'Even recording quality: studio versions, no live noise, jingles or cut-off fragments.',
      ],
    },
    {
      title: 'Caption — what to write',
      text: [
        'A caption of its own for every song, in English, describing the sound: genre, mood, instruments, vocals, timbre, production. A few plain sentences, the way ACE-Step\'s own captioner writes them, or comma-separated tags, or both.',
        'No tempo, key or length in the caption — each song keeps them in fields of their own, measured from the recording. No quoted lyrics and no song title — the lyrics go in their own field.',
        'Do not write the trigger word — the studio puts it in front of every caption itself.',
        'Genre tags are kept apart from the caption. A share of the training steps learns from them alone, so a short prompt calls the style up too.',
      ],
      examples: [
        { label: 'Caption', body: CAPTION_EXAMPLE },
        { label: 'Genre tags', body: GENRE_EXAMPLE },
      ],
    },
    {
      title: 'Auto-describing songs',
      text: [
        'An optional pack (about 9.8 GB), downloaded once from the Auto-describe songs card on the Songs step. With it every song is described by a model that hears it.',
        'Each model loads once for the whole dataset and is let go when its stage is done: the lyric databases, then vocals and recognition for what they miss, then listening, then the assistant for the lyrics. The card holds one model at a time, so a dataset of fifty songs does not take fifty times as long as one.',
      ],
      list: [
        'MOSS-Music-8B listens to a song and names its genre, then describes the sound in a few sentences — the shape ACE-Step\'s captioner uses.',
        'Beat This! finds the beats in the recording and the tempo is worked out from them; S-KEY (Deezer) finds the key. They go into the song\'s tempo and key, not into its caption.',
        'MOSS takes about 12 GB of video memory. Everything runs on your computer, nothing is sent anywhere.',
        'Without the pack a song has a Describe button, and the ⋯ menu has Describe all: the assistant writes a caption from what the field, the title and the lyrics already say. It does not hear the song, so first write briefly what is heard, then check the result by ear.',
      ],
    },
    {
      title: 'Lyrics',
      list: [
        'Exactly the words that are sung, without chords, links or notes. Always check lyrics from websites against the recording.',
        'Mark the parts: [Verse], [Chorus], [Bridge], [Outro], each part on a new line.',
        'A .txt or .lrc with the same name as the audio is taken when the song is added; the time stamps of an .lrc are removed.',
        'Where there are no lyrics, the studio looks them up in open lyrics databases — LRCLIB, QQ Music, Kugou — by artist, title and length. Only when no database knows the song are the vocals separated and the words recognised by Whisper, which is far less accurate. Above the lyrics it says where they came from. Always check the result.',
        'If the recogniser hears no words, the song is marked instrumental and trains as [Instrumental]. If that is wrong, untick Instrumental and press Find again.',
      ],
    },
    {
      title: 'What the studio does itself',
      list: [
        'Converts every song to 48 kHz stereo and writes the trainer\'s dataset: caption, genre, lyrics, tempo, key, time signature and language of each song.',
        'Encodes the songs once with the model\'s VAE and text encoder; the training then runs on that cache.',
        'Puts the trigger word in front of every caption, and into the caption at generation when you pick this LoRA.',
        'Turns the planner\'s audio codes off when you generate with a sound LoRA, as ACE-Step\'s authors advise for a trained LoRA.',
      ],
    },
    {
      title: 'What you have to do yourself',
      list: [
        'Choose the songs and check their quality.',
        'Check the captions and lyrics the studio wrote: the model and the recognition make mistakes. Without the auto-describe pack, give every song a caption yourself or with Describe.',
        'Judge the result by ear; the loss graph cannot do it for you.',
      ],
    },
    {
      title: 'Run settings',
      text: ['Leave the defaults alone without a reason. Every one of them is under Advanced.'],
      list: [
        'Up to 500 epochs; one epoch is one pass over every song. The run stops earlier once the smoothed loss falls to 0.3 and keeps its best adapter; set the target to 0 to train every epoch.',
        'LoKR, dimension 512, alpha 512, factor 6, on attention and the feed-forward layers — the default of HOT-Step, the trainer the studio runs; LoRA of rank 128 and alpha 256 is the other choice. Each takes HOT-Step\'s own learning rate, weight decay and loss weighting.',
        'Prodigy, which finds its own step size; AdamW and Muon take the learning rate (2e-3 for LoKR, 5e-4 for LoRA unless you set one). Gradients are summed over 4 steps before each update.',
        '30% of the steps learn from the genre tags alone, 15% with no caption at all, which guidance at generation needs.',
        'Flash attention keeps memory low on long songs; exact attention needs more.',
      ],
    },
    {
      title: 'Generating with it and training further',
      list: [
        'Press To LoRA under the checkpoint — the LoRA appears on the LoRA page and in the LoRA card of the create form.',
        'When you pick the LoRA, its trigger word is added to the caption by itself. The strength is 1 by default; if the songs come out overdone, start at 0.5–0.75.',
        'Not there yet? Train further: set more epochs in all and the run goes on from the adapter it left, with the same recipe, songs and model. A new checkpoint is added beside the old ones, so you can compare them by ear.',
      ],
    },
    {
      title: 'When something is wrong',
      list: [
        'Songs loop or lose their endings — too many epochs: take an earlier checkpoint, lower the strength, or train again with fewer epochs or a higher target loss.',
        'The LoRA changes almost nothing — train further, check the captions, add songs.',
        'The LoRA is marked for the other size of model — switch the DiT to that size, or train again on the one you use.',
        'Out of video memory — close whatever uses the card; keep flash attention on.',
      ],
    },
    {
      title: 'Checklist before a run',
      checklist: [
        'Songs of one artist or album, 5–20 of them, of good quality, whole.',
        'No duplicates or fragments.',
        'Every song has its own caption, checked against the song.',
        'No lyric quotes, song titles, tempo or key in the captions.',
        'Lyrics checked against the recording and marked [Verse] / [Chorus].',
        'Instrumentals ticked, songs with vocals unticked.',
        'A rare trigger word is set.',
        'The DiT you want the LoRA for is the one selected, and generation is stopped.',
      ],
    },
  ],
};

const ru: Guide = {
  title: 'Справка по обучению LoRA',
  intro: 'LoRA — небольшое дополнение к модели, которое учится звучать как ваши песни: тембр, вокал, аранжировки, продакшн. В ACE-Step Studio учится DiT — часть модели, которая рендерит звук. Окно можно двигать за заголовок и растягивать за правый нижний угол.',
  expandAll: 'Раскрыть всё',
  collapseAll: 'Свернуть всё',
  close: 'Закрыть',
  resize: 'Потяните, чтобы изменить размер',
  sections: [
    {
      title: 'Порядок работы',
      steps: [
        'Перетащите папку с песнями одного исполнителя, лучше одного альбома, в зону на странице «Обучение» или выберите папку и файлы кнопками. Студия сама создаст набор с именем папки. Подходят WAV, MP3, FLAC, OGG, M4A; альбом одним файлом с .cue режется на песни, текст из .txt или .lrc рядом берётся как есть.',
        'Шаг «1 · Песни»: подготовка начинается сама. Студия берёт тексты из баз, отделяет вокал и распознаёт только те, что не нашлись, а с пакетом автоописания слушает каждую песню и пишет её описание и теги жанра с измеренными темпом и тональностью. У каждой песни свой статус, общий ход — сверху.',
        'Каждая песня хранит, на чём остановилась. Если студию закрыть или она упадёт посреди подготовки, после запуска работа продолжится сама с того же места, сделанное не повторяется. У песни с ошибкой есть кнопка «Повторить», у недоделанной — «Доделать»: они доделывают только эту песню.',
        'Проверьте результат: нажмите на песню — она раскроется с плеером, описанием и текстом. Поправьте, что нужно; «Описать заново» и «Найти заново» переделывают только эту песню.',
        'Шаг «2 · Обучение»: название LoRA, слово-триггер (студия сама делает редкое слово из названия набора; его можно поменять или стереть), проверка готовности и кнопка «Обучить». Тренер (около 0,1 ГБ) и неквантованные веса BF16 того DiT, которым вы рендерите (4,5 ГБ для стандартной turbo-модели), скачиваются здесь же, один раз.',
        'Можно не ждать: пока идёт подготовка, поставьте галочку «Начать обучение самому, когда все песни будут готовы».',
        'Шаг «3 · Результат»: прогон оставляет в конце один адаптер — лучший, если он остановился на целевом loss. Послушайте и нажмите «В LoRA»; LoRA появится на странице LoRA. Пока идёт обучение, генерация, ассистент, караоке и разделение на стемы недоступны.',
      ],
    },
    {
      title: 'Какая модель учится',
      list: [
        'Тот DiT, которым студия рендерит сейчас, — его веса BF16: тренер не учит квантованные. Поменяйте модель переключателем над формой создания до обучения.',
        'Адаптер подходит к любому кванту этой модели: учим на BF16, генерируем на Q4 или Q8.',
        'Только к этому размеру модели: адаптер стандартного DiT 2B не вливается в XL, и наоборот. На странице LoRA отмечено, для какого размера каждый адаптер.',
        'Прогон, который учится дальше, держится своей модели: чтобы продолжить, переключитесь на неё обратно.',
      ],
    },
    {
      title: 'Какие песни брать',
      list: [
        'Один исполнитель, лучше один альбом или одна эпоха: похожесть важнее разнообразия.',
        'Обычно 5–20 песен, целиком, с началом и концом.',
        'Ровное качество записи: студийные версии, без концертного шума, джинглов и обрезков.',
      ],
    },
    {
      title: 'Описание — что писать',
      text: [
        'У каждой песни своё описание, на английском, про звук: жанр, настроение, инструменты, вокал, тембр, продакшн. Несколько простых предложений, как пишет родной кэпшенер ACE-Step, или теги через запятую, или и то и другое.',
        'Без темпа, тональности и длины — у каждой песни они в своих полях, измеренные по записи. Без цитат из текста и названия песни — текст идёт в своё поле.',
        'Слово-триггер не пишите: студия сама ставит его перед каждым описанием.',
        'Теги жанра хранятся отдельно от описания. Часть шагов обучения учится только на них, чтобы стиль отзывался и на короткий промпт.',
      ],
      examples: [
        { label: 'Описание', body: CAPTION_EXAMPLE },
        { label: 'Теги жанра', body: GENRE_EXAMPLE },
      ],
    },
    {
      title: 'Автоописание песен',
      text: [
        'Необязательный пакет (около 9,8 ГБ) скачивается один раз с карточки «Автоописание песен» на шаге «Песни». С ним каждую песню описывает модель, которая её слышит.',
        'Каждая модель загружается один раз на весь набор и выгружается, когда её этап закончен: базы текстов, потом вокал и распознавание для того, чего нет в базах, потом прослушивание, потом ассистент для текстов. На видеокарте одновременно одна модель, поэтому набор из пятидесяти песен готовится не в пятьдесят раз дольше одной.',
      ],
      list: [
        'MOSS-Music-8B слушает песню, называет её жанр и описывает звучание в нескольких предложениях — в той форме, в какой пишет кэпшенер ACE-Step.',
        'Beat This! находит доли в записи, по ним считается темп; S-KEY (Deezer) находит тональность. Они идут в темп и тональность песни, а не в описание.',
        'MOSS занимает около 12 ГБ видеопамяти. Всё работает на вашем компьютере, ничего никуда не отправляется.',
        'Без пакета у песни есть кнопка «Описать», а в меню ⋯ — «Описать все»: ассистент пишет описание по тому, что уже есть в поле, названию и тексту. Песню он не слышит, поэтому сначала коротко напишите, что слышно, а потом проверьте результат на слух.',
      ],
    },
    {
      title: 'Тексты',
      list: [
        'Ровно те слова, что поются, без аккордов, ссылок и пометок. Тексты с сайтов всегда сверяйте с записью.',
        'Размечайте части: [Verse], [Chorus], [Bridge], [Outro], каждая часть с новой строки.',
        '.txt или .lrc с тем же именем, что у аудио, берётся при добавлении песни; метки времени из .lrc убираются.',
        'Где текста нет, студия ищет его в открытых базах — LRCLIB, QQ Music, Kugou — по исполнителю, названию и длине. Только если песни нет ни в одной базе, вокал отделяется и слова распознаёт Whisper — гораздо менее точно. Над текстом написано, откуда он. Результат всегда проверяйте.',
        'Если распознавание не слышит слов, песня отмечается инструменталом и учится как [Instrumental]. Если это не так, снимите галочку «Инструментал» и нажмите «Найти заново».',
      ],
    },
    {
      title: 'Что студия делает сама',
      list: [
        'Переводит каждую песню в стерео 48 кГц и пишет набор для тренера: описание, жанр, текст, темп, тональность, размер и язык каждой песни.',
        'Один раз кодирует песни VAE и текстовым энкодером модели; дальше обучение идёт по этому кэшу.',
        'Ставит слово-триггер перед каждым описанием, а при генерации — в описание, когда вы выбираете эту LoRA.',
        'Выключает аудиокоды планировщика, когда вы генерируете со звуковой LoRA, как советуют авторы ACE-Step для обученной LoRA.',
      ],
    },
    {
      title: 'Что нужно сделать самому',
      list: [
        'Выбрать песни и проверить их качество.',
        'Проверить описания и тексты, которые написала студия: модель и распознавание ошибаются. Без пакета автоописания дайте каждой песне описание сами или кнопкой «Описать».',
        'Оценить результат на слух: график loss этого за вас не сделает.',
      ],
    },
    {
      title: 'Настройки прогона',
      text: ['Без причины настройки по умолчанию не трогайте. Все они — в «Дополнительно».'],
      list: [
        'До 500 эпох; эпоха — один проход по всем песням. Прогон остановится раньше, когда сглаженный loss опустится до 0,3, и оставит лучший адаптер; 0 — пройти все эпохи.',
        'LoKR, размерность 512, альфа 512, фактор 6, на внимании и слоях прямого прохода — так по умолчанию учит HOT-Step, тренер, который запускает студия; другой вариант — LoRA ранга 128 и альфа 256. Каждый берёт у HOT-Step свои скорость, weight decay и взвешивание loss.',
        'Prodigy сам подбирает шаг обучения; AdamW и Muon берут скорость (2e-3 для LoKR, 5e-4 для LoRA, если не задать свою). Градиенты копятся 4 шага перед каждым обновлением.',
        '30% шагов учатся только на тегах жанра, 15% — вовсе без описания: это нужно для guidance при генерации.',
        'Flash-внимание держит память низкой на длинных песнях; точное требует больше.',
      ],
    },
    {
      title: 'Генерация с LoRA и дообучение',
      list: [
        'Нажмите «В LoRA» под чекпоинтом — LoRA появится на странице LoRA и в карточке LoRA формы создания.',
        'Когда вы выбираете LoRA, её слово-триггер само добавляется в описание. Сила по умолчанию 1; если песни выходят пережаренными, начните с 0,5–0,75.',
        'Мало? «Учить дальше»: задайте больше эпох всего, и прогон продолжится с адаптера, который оставил, с тем же рецептом, песнями и моделью. Новый чекпоинт появится рядом со старыми — их можно сравнить на слух.',
      ],
    },
    {
      title: 'Если что-то не так',
      list: [
        'Песни зацикливаются или теряют концовки — эпох слишком много: возьмите чекпоинт раньше, уменьшите силу или обучите заново с меньшим числом эпох или более высоким целевым loss.',
        'LoRA почти ничего не меняет — учите дальше, проверьте описания, добавьте песен.',
        'LoRA отмечена для другого размера модели — переключите DiT на этот размер или обучите заново на той модели, которой пользуетесь.',
        'Не хватает видеопамяти — закройте всё, что занимает видеокарту; не выключайте flash-внимание.',
      ],
    },
    {
      title: 'Проверка перед запуском',
      checklist: [
        'Песни одного исполнителя или альбома, 5–20 штук, хорошего качества, целиком.',
        'Нет дублей и обрезков.',
        'У каждой песни своё описание, сверенное с песней.',
        'В описаниях нет цитат из текста, названий, темпа и тональности.',
        'Тексты сверены с записью и размечены [Verse] / [Chorus].',
        'Инструменталы отмечены, у песен с вокалом галочка снята.',
        'Задано редкое слово-триггер.',
        'Выбран тот DiT, для которого нужна LoRA, генерация остановлена.',
      ],
    },
  ],
};

const zh: Guide = {
  title: 'LoRA 训练指南',
  intro: 'LoRA 是模型的一个小附加件，它学习像你的歌曲那样发声：音色、人声、编曲、制作。在 ACE-Step Studio 中学习的是 DiT——模型中负责渲染声音的部分。拖动标题可移动此窗口，拖动右下角可调整大小。',
  expandAll: '全部展开',
  collapseAll: '全部折叠',
  close: '关闭',
  resize: '拖动以调整大小',
  sections: [
    {
      title: '工作流程',
      steps: [
        '把同一位歌手（最好是同一张专辑）的歌曲文件夹拖到“训练”页面，或用按钮选择文件夹或文件。工作室会以文件夹名创建数据集。支持 WAV、MP3、FLAC、OGG、M4A；带 .cue 的整张专辑文件会被切成单曲，歌曲旁的 .txt 或 .lrc 歌词会按原样使用。',
        '第 1 步 · 歌曲：准备会自动开始。工作室从数据库获取歌词，只对找不到的歌曲分离人声并识别；装有自动描述包时，还会聆听每首歌，写出描述和流派标签，并测出速度和调性。每首歌显示自己的状态，总体进度在上方。',
        '每首歌都记得自己停在哪里。如果准备过程中关闭工作室或它崩溃了，下次启动会从同一位置自动继续，已完成的不会重做。失败的歌曲有“重试”按钮，未完成的有“完成这首”：它们只处理这一首。',
        '检查结果：点击歌曲会展开播放器、描述和歌词。修改需要的地方；“重新描述”和“重新查找”只重做这一首。',
        '第 2 步 · 训练：LoRA 名称、触发词（工作室会用数据集名生成一个罕见词，可修改或清空）、就绪检查和“训练”按钮。训练器（约 0.1 GB）和你当前用于渲染的 DiT 的未量化 BF16 权重（标准 turbo 模型为 4.5 GB）在此处下载，只需一次。',
        '不必等待：准备进行时，勾选“所有歌曲就绪后自动开始训练”。',
        '第 3 步 · 结果：一次训练在结束时留下一个适配器——若在目标 loss 处停止，则是最好的那个。试听后点“加入 LoRA”，它会出现在 LoRA 页面。训练期间无法生成、使用助手、卡拉 OK 和分轨。',
      ],
    },
    {
      title: '训练哪个模型',
      list: [
        '工作室当前用于渲染的 DiT——它的 BF16 权重，因为训练器不训练量化权重。训练前请用创作表单上方的切换器更换模型。',
        '适配器适用于该模型的任何量化：在 BF16 上训练，用 Q4 或 Q8 生成。',
        '只适用于该尺寸的模型：标准 2B DiT 的适配器不能合并进 XL，反之亦然。LoRA 页面会标出每个适配器对应的尺寸。',
        '继续训练的运行保持原来的模型：要继续，请切换回那个模型。',
      ],
    },
    {
      title: '用哪些歌曲',
      list: [
        '同一位歌手，最好是同一张专辑或同一时期：相似度比多样性更重要。',
        '通常 5 到 20 首，完整的歌曲，有开头也有结尾。',
        '录音质量一致：录音室版本，没有现场噪声、广告音效或残缺片段。',
      ],
    },
    {
      title: '描述——写什么',
      text: [
        '每首歌都有自己的英文描述，描写声音：流派、情绪、乐器、人声、音色、制作。可以像 ACE-Step 自带的描述器那样写几句平实的句子，也可以是逗号分隔的标签，或两者兼有。',
        '描述中不写速度、调性和时长——每首歌把它们保存在各自的字段里，由录音测得。不引用歌词，不写歌名——歌词有自己的字段。',
        '不要写触发词——工作室会自动把它放在每条描述前面。',
        '流派标签与描述分开保存。一部分训练步骤只用它们学习，这样简短的提示也能唤起风格。',
      ],
      examples: [
        { label: '描述', body: CAPTION_EXAMPLE },
        { label: '流派标签', body: GENRE_EXAMPLE },
      ],
    },
    {
      title: '自动描述歌曲',
      text: [
        '可选包（约 9.8 GB），在“歌曲”步骤的“自动描述歌曲”卡片中下载一次。有了它，每首歌都由能听见它的模型来描述。',
        '每个模型只为整个数据集加载一次，阶段完成后释放：先是歌词数据库，然后对缺失的部分分离人声并识别，然后聆听，最后由助手处理歌词。显卡同时只放一个模型，所以五十首歌的数据集不会比一首慢五十倍。',
      ],
      list: [
        'MOSS-Music-8B 聆听歌曲，说出流派，再用几句话描述声音——就是 ACE-Step 描述器使用的形式。',
        'Beat This! 找出录音中的节拍并据此算出速度；S-KEY（Deezer）找出调性。它们写入歌曲的速度和调性字段，而不是描述。',
        'MOSS 约占 12 GB 显存。一切都在你的电脑上运行，不会发送到任何地方。',
        '没有这个包时，每首歌有“描述”按钮，⋯ 菜单里有“全部描述”：助手根据字段、歌名和歌词中已有的内容写描述。它听不到歌曲，所以先简单写下听到的内容，再用耳朵检查结果。',
      ],
    },
    {
      title: '歌词',
      list: [
        '只写实际唱出的词，不要和弦、链接或备注。网站上的歌词务必对照录音检查。',
        '标出段落：[Verse]、[Chorus]、[Bridge]、[Outro]，每段另起一行。',
        '与音频同名的 .txt 或 .lrc 会在添加歌曲时读取；.lrc 的时间戳会被去掉。',
        '没有歌词时，工作室按歌手、歌名和时长在开放歌词库中查找——LRCLIB、QQ 音乐、酷狗。只有所有数据库都找不到时，才分离人声并用 Whisper 识别，准确度低得多。歌词上方会注明来源。请务必检查结果。',
        '如果识别听不到歌词，歌曲会被标为纯音乐，并以 [Instrumental] 训练。如果标错了，取消“纯音乐”并点“重新查找”。',
      ],
    },
    {
      title: '工作室自动完成的事',
      list: [
        '把每首歌转为 48 kHz 立体声，并为训练器写出数据集：每首歌的描述、流派、歌词、速度、调性、拍号和语言。',
        '用模型的 VAE 和文本编码器把歌曲编码一次；之后训练都基于这份缓存。',
        '把触发词放在每条描述前面；生成时选中这个 LoRA，也会把它加入描述。',
        '用声音 LoRA 生成时关闭规划器的音频编码，这是 ACE-Step 作者对训练好的 LoRA 的建议。',
      ],
    },
    {
      title: '需要你自己做的事',
      list: [
        '挑选歌曲并检查质量。',
        '检查工作室写的描述和歌词：模型和识别都会出错。没有自动描述包时，自己或用“描述”为每首歌写描述。',
        '用耳朵判断结果；loss 曲线替你做不了这件事。',
      ],
    },
    {
      title: '训练设置',
      text: ['没有理由就不要改默认值。所有设置都在“高级”中。'],
      list: [
        '最多 500 个 epoch；一个 epoch 就是把所有歌曲过一遍。平滑后的 loss 降到 0.3 时提前停止，并保留最好的适配器；目标设为 0 则跑满所有 epoch。',
        'LoKR，维度 512、alpha 512、因子 6，作用于注意力层和前馈层——这是工作室所用训练器 HOT-Step 的默认设置；另一种选择是秩 128、alpha 256 的 LoRA。两者都采用 HOT-Step 各自的学习率、权重衰减和 loss 加权。',
        'Prodigy 会自己找步长；AdamW 和 Muon 使用学习率（未设定时 LoKR 为 2e-3，LoRA 为 5e-4）。每次更新前累积 4 步梯度。',
        '30% 的步骤只用流派标签学习，15% 完全不带描述——生成时的 guidance 需要这一点。',
        'Flash 注意力在长歌曲上保持低显存；精确注意力需要更多。',
      ],
    },
    {
      title: '用它生成与继续训练',
      list: [
        '在检查点下点“加入 LoRA”——LoRA 会出现在 LoRA 页面和创作表单的 LoRA 卡片中。',
        '选中 LoRA 时，它的触发词会自动加入描述。强度默认为 1；如果歌曲听起来过头，从 0.5–0.75 开始。',
        '还不够？“继续训练”：设定更多的总 epoch，运行会从它留下的适配器继续，使用相同的配方、歌曲和模型。新的检查点会出现在旧的旁边，可以用耳朵比较。',
      ],
    },
    {
      title: '出问题时',
      list: [
        '歌曲循环或失去结尾——epoch 太多：换更早的检查点、降低强度，或用更少的 epoch 或更高的目标 loss 重新训练。',
        'LoRA 几乎没有变化——继续训练，检查描述，增加歌曲。',
        'LoRA 标为另一种尺寸的模型——把 DiT 切换到该尺寸，或在你使用的模型上重新训练。',
        '显存不足——关闭占用显卡的程序；保持 Flash 注意力开启。',
      ],
    },
    {
      title: '训练前检查清单',
      checklist: [
        '同一位歌手或专辑的歌曲，5–20 首，质量好，完整。',
        '没有重复或残缺片段。',
        '每首歌都有自己的描述，并已对照歌曲检查。',
        '描述中没有歌词引用、歌名、速度和调性。',
        '歌词已对照录音检查，并标注 [Verse] / [Chorus]。',
        '纯音乐已勾选，有人声的歌曲未勾选。',
        '已设定罕见的触发词。',
        '已选中要训练 LoRA 的那个 DiT，生成已停止。',
      ],
    },
  ],
};

const ja: Guide = {
  title: 'LoRA 学習ガイド',
  intro: 'LoRA はモデルへの小さな追加で、あなたの曲のような音を学びます：音色、ボーカル、アレンジ、プロダクション。ACE-Step Studio で学習するのは DiT——モデルの中で音をレンダリングする部分です。タイトルをドラッグしてウィンドウを移動し、右下の角でサイズを変えられます。',
  expandAll: 'すべて開く',
  collapseAll: 'すべて閉じる',
  close: '閉じる',
  resize: 'ドラッグしてサイズ変更',
  sections: [
    {
      title: '作業の流れ',
      steps: [
        '1 人のアーティスト（できれば 1 枚のアルバム）の曲のフォルダーを「学習」ページにドロップするか、ボタンでフォルダーやファイルを選びます。スタジオがフォルダー名でデータセットを作ります。WAV、MP3、FLAC、OGG、M4A に対応。.cue 付きの 1 ファイルのアルバムは曲ごとに分割され、曲の隣の .txt や .lrc の歌詞はそのまま使われます。',
        'ステップ 1 · 曲：準備は自動で始まります。スタジオはデータベースから歌詞を取り、見つからない曲だけボーカルを分離して認識します。自動説明パックがあれば、全曲を聴いてキャプションとジャンルタグを書き、テンポとキーを測定します。曲ごとに状態が表示され、全体の進み具合は上に出ます。',
        '各曲は止まった位置を覚えています。準備中にスタジオを閉じたりクラッシュしたりしても、次の起動で同じところから自動で続き、済んだ作業はやり直しません。失敗した曲には「再試行」、未完了の曲には「この曲を仕上げる」ボタンがあり、その曲だけを処理します。',
        '結果を確認します：曲をクリックするとプレーヤー、キャプション、歌詞が開きます。必要なところを直してください。「もう一度説明」「もう一度探す」はその曲だけをやり直します。',
        'ステップ 2 · 学習：LoRA の名前、トリガーワード（スタジオがデータセット名から珍しい単語を作ります。変更や削除も可能）、準備チェック、「学習」ボタン。トレーナー（約 0.1 GB）と、今レンダリングに使っている DiT の量子化されていない BF16 重み（標準の turbo モデルで 4.5 GB）がここで 1 回だけダウンロードされます。',
        '待つ必要はありません：準備中に「すべての曲が揃ったら自動で学習を始める」にチェックを入れてください。',
        'ステップ 3 · 結果：1 回の学習は最後に 1 つのアダプターを残します——目標 loss で止まった場合は最良のもの。聴いてから「LoRA へ」を押すと、LoRA ページに現れます。学習中は生成、アシスタント、カラオケ、ステム分離は使えません。',
      ],
    },
    {
      title: 'どのモデルを学習するか',
      list: [
        'スタジオが今レンダリングに使っている DiT——その BF16 重みです。トレーナーは量子化された重みを学習しません。学習の前に作成フォームの上の切り替えでモデルを変えてください。',
        'アダプターはそのモデルのどの量子化にも使えます：BF16 で学習し、Q4 や Q8 で生成。',
        '使えるのはそのサイズのモデルだけです：標準 2B の DiT のアダプターは XL には統合できず、その逆も同じです。LoRA ページには各アダプターがどのサイズ用かが表示されます。',
        '続きを学習する実行は元のモデルを保ちます：続けるにはそのモデルに戻してください。',
      ],
    },
    {
      title: 'どの曲を使うか',
      list: [
        '1 人のアーティスト、できれば 1 枚のアルバムか 1 つの時期：幅広さより似ていることが大事です。',
        '普通は 5〜20 曲、始まりから終わりまで丸ごと。',
        '録音品質をそろえる：スタジオ版で、ライブのノイズ、ジングル、途切れた断片のないもの。',
      ],
    },
    {
      title: 'キャプション——何を書くか',
      text: [
        '曲ごとに英語で音を説明するキャプション：ジャンル、ムード、楽器、ボーカル、音色、プロダクション。ACE-Step 自身のキャプショナーのような平易な数文、カンマ区切りのタグ、またはその両方。',
        'テンポ、キー、長さはキャプションに書きません——各曲は録音から測った値を専用の欄に持っています。歌詞の引用や曲名も書きません——歌詞には専用の欄があります。',
        'トリガーワードは書かないでください——スタジオがすべてのキャプションの前に自動で付けます。',
        'ジャンルタグはキャプションとは別に保存されます。学習ステップの一部はそれだけで学ぶので、短いプロンプトでもスタイルが呼び出せます。',
      ],
      examples: [
        { label: 'キャプション', body: CAPTION_EXAMPLE },
        { label: 'ジャンルタグ', body: GENRE_EXAMPLE },
      ],
    },
    {
      title: '曲の自動説明',
      text: [
        'オプションのパック（約 9.8 GB）で、「曲」ステップの「曲を自動で説明」カードから 1 回だけダウンロードします。これがあると、曲を聴けるモデルが各曲を説明します。',
        '各モデルはデータセット全体で 1 回だけ読み込まれ、その段階が終わると解放されます：歌詞データベース、足りない分のボーカル分離と認識、聴き取り、最後に歌詞のためのアシスタント。GPU には同時に 1 つのモデルしか載らないので、50 曲のデータセットが 1 曲の 50 倍かかることはありません。',
      ],
      list: [
        'MOSS-Music-8B が曲を聴いてジャンルを挙げ、音を数文で説明します——ACE-Step のキャプショナーと同じ形です。',
        'Beat This! が録音の拍を見つけてテンポを求め、S-KEY（Deezer）がキーを見つけます。これらはキャプションではなく、曲のテンポとキーの欄に入ります。',
        'MOSS は約 12 GB の VRAM を使います。すべてあなたの PC で動き、どこにも送信されません。',
        'パックがない場合、曲には「説明」ボタン、⋯ メニューには「すべて説明」があります：アシスタントが欄、曲名、歌詞にすでにある内容からキャプションを書きます。曲は聴けないので、まず聞こえるものを短く書き、結果を耳で確かめてください。',
      ],
    },
    {
      title: '歌詞',
      list: [
        '実際に歌われている言葉だけ。コード、リンク、メモは入れません。サイトの歌詞は必ず録音と照らし合わせてください。',
        'パートを示します：[Verse]、[Chorus]、[Bridge]、[Outro]、各パートは新しい行から。',
        '音声と同じ名前の .txt や .lrc は曲の追加時に読み込まれ、.lrc のタイムスタンプは取り除かれます。',
        '歌詞がない場合、スタジオはアーティスト、曲名、長さで公開の歌詞データベース——LRCLIB、QQ Music、Kugou——を探します。どのデータベースにもない曲だけ、ボーカルを分離して Whisper で認識しますが、精度はずっと落ちます。歌詞の上に出どころが表示されます。結果は必ず確認してください。',
        '認識で言葉が聞こえなければ、曲はインストとしてマークされ、[Instrumental] として学習されます。間違いなら「インスト」のチェックを外して「もう一度探す」を押してください。',
      ],
    },
    {
      title: 'スタジオが自動でやること',
      list: [
        '各曲を 48 kHz ステレオに変換し、トレーナー用のデータセットを書きます：各曲のキャプション、ジャンル、歌詞、テンポ、キー、拍子、言語。',
        'モデルの VAE とテキストエンコーダーで曲を 1 回だけエンコードし、学習はそのキャッシュで行います。',
        'すべてのキャプションの前にトリガーワードを付け、生成時にこの LoRA を選ぶとキャプションにも入れます。',
        'サウンド LoRA で生成するときはプランナーのオーディオコードをオフにします。学習済み LoRA について ACE-Step の作者が勧めているとおりです。',
      ],
    },
    {
      title: '自分でやること',
      list: [
        '曲を選び、品質を確認する。',
        'スタジオが書いたキャプションと歌詞を確認する：モデルも認識も間違えます。自動説明パックがなければ、自分で、または「説明」で各曲にキャプションを付けてください。',
        '結果は耳で判断する。loss のグラフは代わりに判断してくれません。',
      ],
    },
    {
      title: '学習の設定',
      text: ['理由がなければ既定値のままにしてください。設定はすべて「詳細」にあります。'],
      list: [
        '最大 500 エポック。1 エポックは全曲を 1 回通すことです。平滑化した loss が 0.3 まで下がると早めに止まり、最良のアダプターを残します。目標を 0 にすると全エポックを学習します。',
        '次元 512、アルファ 512、係数 6 の LoKR を、アテンションとフィードフォワード層に——スタジオが動かすトレーナー HOT-Step の既定です。もう 1 つの選択肢はランク 128、アルファ 256 の LoRA。どちらも HOT-Step のそれぞれの学習率、weight decay、loss の重み付けを使います。',
        'Prodigy はステップ幅を自分で見つけます。AdamW と Muon は学習率を使います（指定しなければ LoKR は 2e-3、LoRA は 5e-4）。各更新の前に 4 ステップ分の勾配を合算します。',
        'ステップの 30% はジャンルタグだけで、15% はキャプションなしで学習します——生成時の guidance に必要です。',
        'Flash アテンションは長い曲でもメモリを抑えます。正確なアテンションはもっと必要です。',
      ],
    },
    {
      title: 'LoRA での生成と追加学習',
      list: [
        'チェックポイントの下の「LoRA へ」を押すと、LoRA ページと作成フォームの LoRA カードに現れます。',
        'LoRA を選ぶと、そのトリガーワードがキャプションに自動で加わります。強さの既定値は 1。やりすぎに聞こえたら 0.5〜0.75 から始めてください。',
        'まだ足りない？「さらに学習」：合計のエポックを増やすと、残したアダプターから同じレシピ、曲、モデルで続きます。新しいチェックポイントが古いものの隣に加わるので、耳で聴き比べられます。',
      ],
    },
    {
      title: 'うまくいかないとき',
      list: [
        '曲がループしたり終わり方を失ったりする——エポックが多すぎます：早いチェックポイントを使う、強さを下げる、またはエポックを減らすか目標 loss を上げて学習し直してください。',
        'LoRA がほとんど何も変えない——さらに学習し、キャプションを確認し、曲を足してください。',
        'LoRA が別のサイズのモデル用と表示される——DiT をそのサイズに切り替えるか、使っているモデルで学習し直してください。',
        'VRAM が足りない——GPU を使っているものを閉じてください。Flash アテンションはオンのままに。',
      ],
    },
    {
      title: '実行前チェックリスト',
      checklist: [
        '1 人のアーティストかアルバムの曲、5〜20 曲、品質がよく、丸ごと。',
        '重複や断片がない。',
        '各曲に自分のキャプションがあり、曲と照らし合わせてある。',
        'キャプションに歌詞の引用、曲名、テンポ、キーがない。',
        '歌詞を録音と照らし合わせ、[Verse] / [Chorus] で区切ってある。',
        'インストにはチェック、ボーカル曲にはチェックなし。',
        '珍しいトリガーワードを設定した。',
        'LoRA を作りたい DiT が選ばれていて、生成は止まっている。',
      ],
    },
  ],
};

const ko: Guide = {
  title: 'LoRA 학습 안내',
  intro: 'LoRA는 모델에 붙는 작은 추가 파일로, 당신의 곡처럼 들리는 법을 배웁니다: 음색, 보컬, 편곡, 프로덕션. ACE-Step Studio에서 배우는 것은 DiT, 즉 모델에서 소리를 렌더링하는 부분입니다. 제목을 끌어 창을 옮기고 오른쪽 아래 모서리로 크기를 바꿀 수 있습니다.',
  expandAll: '모두 펼치기',
  collapseAll: '모두 접기',
  close: '닫기',
  resize: '끌어서 크기 조절',
  sections: [
    {
      title: '작업 순서',
      steps: [
        '한 아티스트(가능하면 한 앨범)의 곡 폴더를 “학습” 페이지에 끌어다 놓거나 버튼으로 폴더나 파일을 고르세요. 스튜디오가 폴더 이름으로 데이터셋을 만듭니다. WAV, MP3, FLAC, OGG, M4A를 지원하며, .cue가 있는 한 파일짜리 앨범은 곡별로 잘리고, 곡 옆의 .txt나 .lrc 가사는 그대로 쓰입니다.',
        '1단계 · 곡: 준비가 저절로 시작됩니다. 스튜디오는 데이터베이스에서 가사를 가져오고, 찾지 못한 곡만 보컬을 분리해 인식합니다. 자동 설명 팩이 있으면 모든 곡을 듣고 캡션과 장르 태그를 쓰며 템포와 키를 측정합니다. 곡마다 상태가 표시되고 전체 진행은 위에 나옵니다.',
        '곡마다 멈춘 위치를 기억합니다. 준비 중에 스튜디오를 닫거나 멈춰도 다음 실행 때 같은 곳에서 저절로 이어지며, 끝난 일은 다시 하지 않습니다. 실패한 곡에는 “다시 시도”, 끝나지 않은 곡에는 “이 곡 마무리” 버튼이 있어 그 곡만 처리합니다.',
        '결과를 확인하세요: 곡을 누르면 플레이어, 캡션, 가사가 펼쳐집니다. 필요한 곳을 고치세요. “다시 설명”과 “다시 찾기”는 그 곡만 다시 합니다.',
        '2단계 · 학습: LoRA 이름, 트리거 단어(스튜디오가 데이터셋 이름으로 드문 단어를 만들며, 바꾸거나 지울 수 있음), 준비 확인, “학습” 버튼. 트레이너(약 0.1 GB)와 지금 렌더링에 쓰는 DiT의 양자화되지 않은 BF16 가중치(표준 turbo 모델은 4.5 GB)를 여기서 한 번 내려받습니다.',
        '기다릴 필요 없습니다: 준비가 진행되는 동안 “모든 곡이 준비되면 학습을 자동으로 시작”을 체크하세요.',
        '3단계 · 결과: 한 번의 학습은 끝에 어댑터 하나를 남깁니다. 목표 loss에서 멈췄다면 가장 좋은 것입니다. 들어 보고 “LoRA로”를 누르면 LoRA 페이지에 나타납니다. 학습 중에는 생성, 어시스턴트, 가라오케, 스템 분리를 쓸 수 없습니다.',
      ],
    },
    {
      title: '어떤 모델을 학습하나',
      list: [
        '스튜디오가 지금 렌더링에 쓰는 DiT의 BF16 가중치입니다. 트레이너는 양자화된 가중치를 학습하지 않습니다. 학습 전에 생성 폼 위의 전환기로 모델을 바꾸세요.',
        '어댑터는 그 모델의 어떤 양자화에도 맞습니다: BF16으로 학습하고 Q4나 Q8로 생성하세요.',
        '그 크기의 모델에만 맞습니다: 표준 2B DiT의 어댑터는 XL에 합칠 수 없고, 반대도 마찬가지입니다. LoRA 페이지에 각 어댑터가 어떤 크기용인지 표시됩니다.',
        '이어서 학습하는 실행은 원래 모델을 유지합니다: 계속하려면 그 모델로 다시 전환하세요.',
      ],
    },
    {
      title: '어떤 곡을 쓸까',
      list: [
        '한 아티스트, 가능하면 한 앨범이나 한 시기: 다양함보다 닮음이 중요합니다.',
        '보통 5~20곡, 시작부터 끝까지 온전한 곡.',
        '고른 녹음 품질: 스튜디오 버전, 라이브 소음, 징글, 잘린 조각 없이.',
      ],
    },
    {
      title: '캡션: 무엇을 쓸까',
      text: [
        '곡마다 영어로 소리를 설명하는 캡션: 장르, 분위기, 악기, 보컬, 음색, 프로덕션. ACE-Step 자체 캡셔너처럼 평범한 몇 문장, 쉼표로 구분한 태그, 또는 둘 다.',
        '템포, 키, 길이는 캡션에 쓰지 않습니다. 각 곡은 녹음에서 측정한 값을 전용 칸에 가지고 있습니다. 가사 인용이나 곡 제목도 쓰지 않습니다. 가사는 전용 칸이 있습니다.',
        '트리거 단어는 쓰지 마세요. 스튜디오가 모든 캡션 앞에 알아서 붙입니다.',
        '장르 태그는 캡션과 따로 저장됩니다. 학습 단계의 일부는 그것만으로 배우므로 짧은 프롬프트로도 스타일을 불러낼 수 있습니다.',
      ],
      examples: [
        { label: '캡션', body: CAPTION_EXAMPLE },
        { label: '장르 태그', body: GENRE_EXAMPLE },
      ],
    },
    {
      title: '곡 자동 설명',
      text: [
        '선택 팩(약 9.8 GB)으로, “곡” 단계의 “곡 자동 설명” 카드에서 한 번 내려받습니다. 이 팩이 있으면 곡을 들을 수 있는 모델이 각 곡을 설명합니다.',
        '각 모델은 데이터셋 전체에 한 번만 불러오고 그 단계가 끝나면 내려놓습니다: 가사 데이터베이스, 부족한 곡의 보컬 분리와 인식, 듣기, 마지막으로 가사를 위한 어시스턴트. GPU에는 한 번에 모델 하나만 올라가므로 50곡 데이터셋이 한 곡의 50배 걸리지 않습니다.',
      ],
      list: [
        'MOSS-Music-8B가 곡을 듣고 장르를 말한 뒤 소리를 몇 문장으로 설명합니다. ACE-Step 캡셔너가 쓰는 형식입니다.',
        'Beat This!가 녹음의 박자를 찾아 템포를 계산하고, S-KEY(Deezer)가 키를 찾습니다. 이 값은 캡션이 아니라 곡의 템포와 키 칸에 들어갑니다.',
        'MOSS는 약 12 GB의 VRAM을 씁니다. 모든 것이 내 컴퓨터에서 돌아가며 어디에도 보내지 않습니다.',
        '팩이 없으면 곡마다 “설명” 버튼이, ⋯ 메뉴에는 “모두 설명”이 있습니다: 어시스턴트가 칸, 제목, 가사에 이미 있는 내용으로 캡션을 씁니다. 곡을 듣지는 못하므로 먼저 들리는 것을 짧게 적고, 결과를 귀로 확인하세요.',
      ],
    },
    {
      title: '가사',
      list: [
        '실제로 부르는 말만, 코드, 링크, 메모 없이. 사이트의 가사는 항상 녹음과 대조하세요.',
        '파트를 표시하세요: [Verse], [Chorus], [Bridge], [Outro], 파트마다 새 줄에서.',
        '오디오와 같은 이름의 .txt나 .lrc는 곡을 추가할 때 읽히고, .lrc의 타임스탬프는 지워집니다.',
        '가사가 없으면 스튜디오가 아티스트, 제목, 길이로 공개 가사 데이터베이스(LRCLIB, QQ Music, Kugou)를 찾습니다. 어느 데이터베이스에도 없는 곡만 보컬을 분리해 Whisper로 인식하는데, 정확도가 훨씬 낮습니다. 가사 위에 출처가 표시됩니다. 결과는 항상 확인하세요.',
        '인식이 말을 듣지 못하면 곡은 인스트루멘털로 표시되어 [Instrumental]로 학습됩니다. 틀렸다면 “인스트루멘털” 체크를 풀고 “다시 찾기”를 누르세요.',
      ],
    },
    {
      title: '스튜디오가 알아서 하는 일',
      list: [
        '모든 곡을 48 kHz 스테레오로 바꾸고 트레이너용 데이터셋을 씁니다: 곡마다 캡션, 장르, 가사, 템포, 키, 박자, 언어.',
        '모델의 VAE와 텍스트 인코더로 곡을 한 번만 인코딩하고, 학습은 그 캐시로 진행합니다.',
        '모든 캡션 앞에 트리거 단어를 붙이고, 생성 때 이 LoRA를 고르면 캡션에도 넣습니다.',
        '사운드 LoRA로 생성할 때는 플래너의 오디오 코드를 끕니다. 학습한 LoRA에 대해 ACE-Step 제작진이 권하는 방법입니다.',
      ],
    },
    {
      title: '직접 해야 하는 일',
      list: [
        '곡을 고르고 품질을 확인하기.',
        '스튜디오가 쓴 캡션과 가사 확인하기: 모델도 인식도 틀립니다. 자동 설명 팩이 없으면 직접 또는 “설명”으로 곡마다 캡션을 붙이세요.',
        '결과는 귀로 판단하기. loss 그래프가 대신해 주지 않습니다.',
      ],
    },
    {
      title: '학습 설정',
      text: ['이유 없이 기본값을 바꾸지 마세요. 모든 설정은 “고급”에 있습니다.'],
      list: [
        '최대 500 에포크. 한 에포크는 모든 곡을 한 번 도는 것입니다. 평활한 loss가 0.3까지 내려가면 일찍 멈추고 가장 좋은 어댑터를 남깁니다. 목표를 0으로 두면 모든 에포크를 학습합니다.',
        '차원 512, 알파 512, 인수 6의 LoKR을 어텐션과 피드포워드 층에. 스튜디오가 돌리는 트레이너 HOT-Step의 기본값입니다. 다른 선택은 랭크 128, 알파 256의 LoRA이며, 둘 다 HOT-Step의 각자 학습률, weight decay, loss 가중치를 씁니다.',
        'Prodigy는 스텝 크기를 스스로 찾고, AdamW와 Muon은 학습률을 씁니다(지정하지 않으면 LoKR 2e-3, LoRA 5e-4). 업데이트마다 4스텝의 그래디언트를 더합니다.',
        '스텝의 30%는 장르 태그만으로, 15%는 캡션 없이 배웁니다. 생성 때 guidance에 필요합니다.',
        'Flash 어텐션은 긴 곡에서도 메모리를 낮게 유지합니다. 정확한 어텐션은 더 필요합니다.',
      ],
    },
    {
      title: 'LoRA로 생성하기와 이어서 학습하기',
      list: [
        '체크포인트 아래 “LoRA로”를 누르면 LoRA 페이지와 생성 폼의 LoRA 카드에 나타납니다.',
        'LoRA를 고르면 트리거 단어가 캡션에 저절로 들어갑니다. 강도는 기본 1이며, 곡이 과하게 들리면 0.5~0.75부터 시작하세요.',
        '아직 부족한가요? “이어서 학습”: 전체 에포크를 늘리면 남긴 어댑터에서 같은 레시피, 곡, 모델로 이어집니다. 새 체크포인트가 옛것 옆에 생겨 귀로 비교할 수 있습니다.',
      ],
    },
    {
      title: '문제가 생기면',
      list: [
        '곡이 반복되거나 끝을 잃음: 에포크가 너무 많습니다. 더 이른 체크포인트를 쓰거나 강도를 낮추거나, 에포크를 줄이거나 목표 loss를 높여 다시 학습하세요.',
        'LoRA가 거의 아무것도 바꾸지 않음: 이어서 학습하고, 캡션을 확인하고, 곡을 더하세요.',
        'LoRA가 다른 크기의 모델용으로 표시됨: DiT를 그 크기로 바꾸거나 쓰는 모델에서 다시 학습하세요.',
        'VRAM 부족: GPU를 쓰는 프로그램을 닫으세요. Flash 어텐션은 켜 두세요.',
      ],
    },
    {
      title: '실행 전 체크리스트',
      checklist: [
        '한 아티스트나 앨범의 곡, 5~20곡, 좋은 품질, 온전한 곡.',
        '중복이나 조각이 없음.',
        '곡마다 자기 캡션이 있고 곡과 대조했음.',
        '캡션에 가사 인용, 곡 제목, 템포, 키가 없음.',
        '가사를 녹음과 대조하고 [Verse] / [Chorus]로 표시했음.',
        '인스트루멘털은 체크, 보컬 곡은 체크 해제.',
        '드문 트리거 단어를 정함.',
        'LoRA를 만들 DiT가 선택되어 있고 생성은 멈춤.',
      ],
    },
  ],
};

export const trainingGuide: Record<Language, Guide> = { en, ru, zh, ja, ko };
