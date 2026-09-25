import type { Language } from './translations';

/**
 * The training guide of MiniMax Music3 Studio, per language. It follows the
 * studio's own training pipeline and the notes of the HOT-Step trainer it runs.
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

const CAPTION_SKELETON = `Global Metadata
Basic Attributes: bpm is around 118-124, key is ... (a range is fine when unsure)
Global Emotional Progression: ...
Application Scenarios & Imagery: ...
Sonics & Production Profile: ...

Vocal Details
Vocal Gender & Timbre: Singer A - ...
Vocal Style: ...
Harmony/Backing Vocals: ...
Vocal FX: ...

Arrangement
Instrument Lifecycle Description (Primary/Secondary Layering): ...
Groove & Foundation Progression: ...
Embellishments, Textures & Spatial FX: ...`;

const ru: Guide = {
  title: 'Справка по обучению LoRA',
  intro: 'LoRA — небольшое дополнение к модели, которое учится писать песни как ваши: форма, вокал, аранжировки. В MiniMax учится планировщик — часть модели, которая сочиняет песню. Окно можно двигать за заголовок и растягивать за правый нижний угол.',
  expandAll: 'Раскрыть всё',
  collapseAll: 'Свернуть всё',
  close: 'Закрыть',
  resize: 'Потяните, чтобы изменить размер',
  sections: [
    {
      title: "Порядок работы",
      steps: [
        "Перетащите папку с песнями одного исполнителя, лучше одного альбома, в зону на странице «Обучение» или выберите папку и файлы кнопками. Студия сама создаст набор с именем папки. Подходят WAV, MP3, FLAC, OGG, M4A; альбом одним файлом с .cue режется на песни, текст из .txt или .lrc рядом берётся как есть. Песни длиннее 6 минут тренер не берёт.",
        "Шаг «1 · Песни»: подготовка начинается сама. Студия берёт тексты из баз, отделяет вокал и распознаёт только те, что не нашлись, слушает каждую песню и пишет её описание в трёх частях с измеренными темпом и тональностью. У каждой песни свой статус, общий ход — сверху.",
        "Каждая песня хранит, на чём остановилась. Если студию закрыть или она упадёт посреди подготовки, после запуска работа продолжится сама с того же места, сделанное не повторяется. У песни с ошибкой есть кнопка «Повторить», у недоделанной — «Доделать»: они доделывают только эту песню.",
        "Проверьте результат: нажмите на песню — она раскроется с плеером, описанием и текстом. Поправьте, что нужно; «Описать заново» и «Найти заново» переделывают только эту песню.",
        "Шаг «2 · Обучение»: название LoRA, слово-триггер (студия сама делает редкое слово из названия набора, например nrmnkhffn для «Нейромонах Феофан»; его можно поменять или стереть), проверка готовности и кнопка «Обучить». Файлы обучения (около 11.3 ГБ, один раз) скачиваются здесь же. Нужна NVIDIA RTX 30-й серии или новее с 22 ГБ видеопамяти (RTX 3090, 4090, 5090).",
        "Можно не ждать: пока идёт подготовка, поставьте галочку «Начать обучение самому, когда все песни будут готовы».",
        "Шаг «3 · Результат»: послушайте чекпоинты и нажмите «В LoRA» под лучшим — он появится на странице LoRA. Пока идёт обучение, генерация, ассистент, караоке и разделение на дорожки недоступны.",
      ],
    },
    {
      title: 'Какие песни брать',
      list: [
        'Один исполнитель, лучше один альбом или одна эпоха. Рецепт настроен на «клон альбома»: сходство важнее универсальности.',
        'Обычно берут от 5 до 20 песен. Автор тренера не заметил надёжной разницы между 10 и 20 треками; плотные по тексту или разностилевые альбомы учатся тяжелее.',
        'Ровное качество записей: студийные версии, без концертных шумов, джинглов и обрезанных фрагментов.',
        'Целые песни, с началом и концовкой: модель учит форму песни, включая то, как она заканчивается.',
      ],
    },
    {
      title: 'Описание — что писать',
      text: [
        'Своё описание для каждой песни, на английском, примерно 250–450 слов, в трёх частях: Global Metadata, Vocal Details, Arrangement.',
        'Одно общее описание на весь альбом делать нельзя: у автора тренера с ним песни переставали нормально заканчиваться (0 естественных концовок из 6 против 4 из 6 с описаниями по песням).',
        'Не цитируйте и не пересказывайте текст песни и не пишите её название — текст идёт отдельным полем.',
        'Точные BPM и тональность пишите, только если знаете их (из анализатора или базы треков); иначе диапазон: «bpm is around 118-124».',
        'Слово-триггер писать не нужно — студия сама поставит его первым в Global Metadata.',
      ],
      examples: [{ label: 'Каркас', body: CAPTION_SKELETON }],
    },
    {
      title: "Автоописание песен",
      text: [
        "Необязательный пакет (около 10.5 ГБ), скачивается один раз карточкой «Автоописание песен» на шаге «Песни». С ним описание каждой песни пишет модель, которая её слышит.",
        "Каждая модель загружается один раз на весь набор и выгружается, когда её этап закончен: базы текстов, затем вокал и распознавание для ненайденного, затем прослушивание, затем ассистент для текстов. На видеокарте всегда одна модель. Поэтому набор из пятидесяти песен готовится не в пятьдесят раз дольше одной.",
      ],
      list: [
        "MOSS-Music-8B слушает песню и сразу пишет описание в формате MiniMax: Global Metadata, Vocal Details, Arrangement. Так же описания делает автор тренера HOT-Step.",
        "Beat This! находит удары в записи и по ним считается темп, S-KEY (Deezer) определяет тональность. Числа MOSS в описании всегда заменяются измеренными.",
        "MOSS занимает около 12 ГБ видеопамяти; на RTX 4090 одна песня описывается за 6–7 секунд. Всё работает на вашем компьютере, ничего никуда не отправляется.",
        "Без пакета у песни есть кнопка «Описать», а в меню «⋯» — «Описать все»: ассистент пишет описание из того, что уже есть в поле, названия и текста. Песню он не слышит, поэтому сначала впишите коротко, что звучит, и проверьте результат на слух.",
      ],
    },
    {
      title: "Текст песни",
      list: [
        "Точно те слова, что поются, — без аккордов, ссылок и примечаний. Текст с сайтов обязательно сверьте с записью.",
        "Размечайте части: [verse], [chorus], [bridge], [outro] — каждая часть с новой строки.",
        "Файл .txt или .lrc с тем же именем, что и аудио, подхватывается при добавлении; таймкоды из .lrc убираются.",
        "Если текста нет, студия сама ищет его в открытых базах текстов — LRCLIB, QQ Music, Kugou — по исполнителю, названию и длительности (исполнитель и название берутся из тегов файла или из имени и папок). Только если ни одна база песню не знает, отделяется вокал и текст распознаёт Whisper — это заметно менее точно. Над текстом видно, откуда он взят. Результат всегда проверяйте.",
        "Если распознаватель не услышал слов, песня помечается инструменталом. Если это ошибка, снимите галочку «Инструментал» и нажмите «Найти заново».",
      ],
    },
    {
      title: "Что студия делает сама",
      list: [
        "Переводит всё в WAV и готовит для тренера коды звука.",
        "Ставит слово-триггер первым в Global Metadata каждого описания, а при генерации — в описание, когда вы выбираете эту LoRA.",
        "Оборачивает описание без заголовков в Global Metadata, так что строчка из YuE2 Studio тоже подойдёт.",
        "Сохраняет чекпоинты каждые 100 шагов.",
      ],
    },
    {
      title: "Что нужно сделать самому",
      list: [
        "Подобрать песни и проверить их качество.",
        "Проверить описания и тексты, которые написала студия: модель и распознавание ошибаются. Без пакета автоописания — дать каждой песне описание самому или через «Описать».",
        "Выбрать чекпоинт на слух — по графику ошибки выбирать нельзя.",
      ],
    },
    {
      title: 'Настройки запуска',
      text: ['Настройки по умолчанию — рецепт «Balanced» автора тренера HOT-Step. Без причины их лучше не трогать.'],
      list: [
        'Шагов 600. У автора есть ещё быстрый вариант на 300 и основательный на 900.',
        'Вместо шагов можно выбрать эпохи: одна эпоха — один проход по всем песням набора, число шагов студия считает сама.',
        'HOT-PiZZA, ранг 128, alpha 128, выключение ранга 0.1, AdamW, скорость 8e-5. Скорость не повышайте: у автора все варианты с удвоенной скоростью звучали хуже, один спланировал песню совсем без вокала.',
        'Окно 1536 кадров — около 61 секунды песни за раз. Большее окно лучше учит форму песни и концовки, но просит больше памяти; 9000 — песня целиком, нужна карта на 32 ГБ.',
        'Сохранять каждые 100 шагов.',
      ],
    },
    {
      title: 'Выбор чекпоинта и генерация',
      list: [
        'Выбирайте на слух: сгенерируйте одну и ту же песню с разными чекпоинтами. У автора тренера лучший по графику ошибки чекпоинт был в 1–8 раз раньше того, что звучит правильно.',
        'Чем больше шагов, тем больше сходство, но тем меньше связность: вокал может стать сбивчивым.',
        'Нажмите «В LoRA» под нужным шагом — LoRA появится на странице LoRA с именем «запуск · шаг».',
        'При выборе LoRA на странице «Создать» триггер подставится сам. Сила по умолчанию 1; на «пережаренном» чекпоинте начните с 0.5–0.75.',
        'Пишите текст на целую песню, даже если нужен короткий трек. Неровные по длине строки в манере исполнителя работают лучше аккуратных четверостиший.',
      ],
    },
    {
      title: 'Если что-то не так',
      list: [
        'Песни рано обрываются, гудящие вступления, вокал разваливается — LoRA перетренирована: возьмите более ранний чекпоинт или уменьшите силу. Ошибка обучения ниже примерно 1 — признак заучивания.',
        'LoRA почти ничего не меняет — возьмите более поздний чекпоинт, проверьте описания, добавьте песен.',
        'Песни перестали нормально заканчиваться — проверьте, что у каждой песни своё описание, а не одно на всех.',
        'Не хватает видеопамяти — закройте всё, что занимает видеокарту, или уменьшите окно кадров.',
      ],
    },
    {
      title: 'Чеклист перед запуском',
      checklist: [
        'Песни одного исполнителя или альбома, 5–20 штук, хорошего качества, целиком.',
        'Нет дублей и обрезков, песни не длиннее 6 минут.',
        'У каждой песни своё описание в трёх частях, проверенное на слух.',
        'В описаниях нет цитат из текста и названий песен.',
        'Тексты сверены с записью и размечены [verse] / [chorus].',
        'Инструменталы отмечены галочкой, у песен с вокалом галочка снята.',
        'Задано редкое слово-триггер.',
        'Свободно около 22 ГБ видеопамяти, генерация остановлена.',
      ],
    },
  ],
};

const en: Guide = {
  title: 'LoRA training guide',
  intro: 'A LoRA is a small add-on to the model that learns to write songs like yours: the form, the vocals, the arrangements. In MiniMax the planner is what learns — the part of the model that composes the song. Drag this window by its title and resize it from the bottom-right corner.',
  expandAll: 'Expand all',
  collapseAll: 'Collapse all',
  close: 'Close',
  resize: 'Drag to resize',
  sections: [
    {
      title: "Workflow",
      steps: [
        "Drop a folder of songs by one artist, best of one album, onto the Training page, or pick a folder or files with the buttons. The studio makes a dataset named after the folder. WAV, MP3, FLAC, OGG and M4A work; an album in one file with a .cue is cut into its songs, lyrics in a .txt or .lrc beside a song are taken as they are. The trainer does not take songs longer than 6 minutes.",
        "Step 1 · Songs: preparation starts by itself. The studio takes the lyrics from the databases, separates the vocals and recognises only the songs they do not know, listens to every song and writes its three-part caption with the measured tempo and key. Every song shows its own status, the overall progress is on top.",
        "Every song keeps where it stopped. If the studio is closed or crashes during preparation, the work carries on by itself from the same place at the next start, and nothing done is done again. A song that failed has a Retry button, an unfinished one Finish this song: they finish just that song.",
        "Check the result: click a song and it opens with a player, its caption and its lyrics. Fix what needs fixing; Describe again and Find again redo just that song.",
        "Step 2 · Training: the LoRA name, a trigger word (the studio makes a rare word from the dataset name, such as nrmnkhffn for \"Нейромонах Феофан\"; change or clear it as you like), the readiness check and the Train button. The training files (about 11.3 GB, once) download right there. It needs an NVIDIA RTX 30-series card or newer with 22 GB of video memory (RTX 3090, 4090, 5090).",
        "No need to wait: while preparation runs, tick \"Start training by itself when every song is ready\".",
        "Step 3 · Result: listen to the checkpoints and press To LoRA under the best one; it appears on the LoRA page. While training runs, generation, the assistant, karaoke and stem separation are unavailable.",
      ],
    },
    {
      title: 'Which songs to use',
      list: [
        'One artist, ideally one album or one era. The recipe is tuned to clone an album: likeness matters more than range.',
        'Usually 5 to 20 songs. The trainer\'s author saw no reliable difference between 10 and 20 tracks; lyric-dense or mixed-style albums are harder to learn.',
        'Even recording quality: studio versions, no live noise, jingles or cut-off fragments.',
        'Whole songs, with their start and end: the model learns the form of a song, including how it ends.',
      ],
    },
    {
      title: 'Caption — what to write',
      text: [
        'A caption of its own for every song, in English, about 250–450 words, in three parts: Global Metadata, Vocal Details, Arrangement.',
        'Do not use one caption for the whole album: for the trainer\'s author it stopped songs from ending properly (0 natural endings out of 6, against 4 out of 6 with per-song captions).',
        'Do not quote or paraphrase the lyrics and do not name the song — the lyrics go in their own field.',
        'Give an exact BPM and key only when you know them (from an analyser or a track database); otherwise a range: "bpm is around 118-124".',
        'Do not write the trigger word — the studio puts it first in Global Metadata itself.',
      ],
      examples: [{ label: 'Skeleton', body: CAPTION_SKELETON }],
    },
    {
      title: "Auto-describing songs",
      text: [
        "An optional pack (about 10.5 GB), downloaded once from the Auto-describe songs card on the Songs step. With it every song's caption is written by a model that hears it.",
        "Each model loads once for the whole dataset and is let go when its stage is done: the lyric databases, then vocals and recognition for what they miss, then listening, then the assistant for the lyrics. The card holds one model at a time. So a dataset of fifty songs does not take fifty times as long as one.",
      ],
      list: [
        "MOSS-Music-8B listens to a song and writes its caption in MiniMax's format at once: Global Metadata, Vocal Details, Arrangement. The author of the HOT-Step trainer makes captions the same way.",
        "Beat This! finds the beats in the recording and the tempo is worked out from them; S-KEY (Deezer) finds the key. The numbers MOSS puts in the caption are always replaced with the measured ones.",
        "MOSS takes about 12 GB of video memory; on an RTX 4090 a song is described in 6–7 seconds. Everything runs on your computer, nothing is sent anywhere.",
        "Without the pack a song has a Describe button, and the ⋯ menu has Describe all: the assistant writes a caption from what the field, the title and the lyrics already say. It does not hear the song, so first write briefly what is heard, then check the result by ear.",
      ],
    },
    {
      title: "Lyrics",
      list: [
        "Exactly the words that are sung, without chords, links or notes. Always check lyrics from websites against the recording.",
        "Mark the parts: [verse], [chorus], [bridge], [outro], each part on a new line.",
        "A .txt or .lrc with the same name as the audio is taken when the song is added; the time stamps of an .lrc are removed.",
        "Where there are no lyrics, the studio looks them up in open lyrics databases - LRCLIB, QQ Music, Kugou - by artist, title and length (artist and title come from the file's tags, or its name and folders). Only when no database knows the song are the vocals separated and the words recognised by Whisper, which is far less accurate. Above the lyrics it says where they came from. Always check the result.",
        "If the recogniser hears no words, the song is marked instrumental. If that is wrong, untick Instrumental and press Find again.",
      ],
    },
    {
      title: "What the studio does itself",
      list: [
        "Converts everything to WAV and prepares the sound codes for the trainer.",
        "Puts the trigger word first in the Global Metadata of every caption, and into the caption at generation when you pick this LoRA.",
        "Wraps a caption without headings in Global Metadata, so a line from YuE2 Studio works too.",
        "Saves checkpoints every 100 steps.",
      ],
    },
    {
      title: "What you have to do yourself",
      list: [
        "Choose the songs and check their quality.",
        "Check the captions and lyrics the studio wrote: the model and the recognition make mistakes. Without the auto-describe pack, give every song a caption yourself or with Describe.",
        "Choose a checkpoint by ear; the loss graph cannot choose it for you.",
      ],
    },
    {
      title: 'Run settings',
      text: ['The defaults are the Balanced recipe of the HOT-Step trainer\'s author. Leave them alone without a reason.'],
      list: [
        '600 steps. The author also has a fast variant of 300 and a thorough one of 900.',
        'Instead of steps you can choose epochs: one epoch is one pass over every song of the dataset, and the studio works out the steps.',
        'HOT-PiZZA, rank 128, alpha 128, rank dropout 0.1, AdamW, learning rate 8e-5. Do not raise the rate: for the author every doubled-rate variant sounded worse, and one planned a song with no vocals at all.',
        'A window of 1536 frames — about 61 seconds of a song at a time. A larger window teaches the song form and endings better but needs more memory; 9000 is the whole song and needs a 32 GB card.',
        'Save every 100 steps.',
      ],
    },
    {
      title: 'Choosing a checkpoint and generating',
      list: [
        'Choose by ear: generate the same song with different checkpoints. For the trainer\'s author the checkpoint with the best loss came 1–8 times earlier than the one that sounded right.',
        'More steps bring more likeness but less coherence: the vocal can start to stumble.',
        'Press "To LoRA" under the step you want — the LoRA appears on the LoRA page as "run · step".',
        'When you pick the LoRA on the Create page, the trigger is added by itself. The strength is 1 by default; with an over-baked checkpoint start at 0.5–0.75.',
        'Write lyrics for a whole song even when you want a short track. Lines of uneven length in the artist\'s manner work better than tidy quatrains.',
      ],
    },
    {
      title: 'When something is wrong',
      list: [
        'Songs end early, intros drone, the vocal falls apart — the LoRA is overtrained: take an earlier checkpoint or lower the strength. A training loss below about 1 is a sign of memorising.',
        'The LoRA changes almost nothing — take a later checkpoint, check the captions, add songs.',
        'Songs stopped ending properly — check that every song has its own caption, not one shared by all.',
        'Out of VRAM — close whatever uses the card, or lower the frame window.',
      ],
    },
    {
      title: 'Checklist before a run',
      checklist: [
        'Songs of one artist or album, 5–20 of them, of good quality, whole.',
        'No duplicates or fragments, no song longer than 6 minutes.',
        'Every song has its own three-part caption, checked against the song.',
        'No lyric quotes or song titles in the captions.',
        'Lyrics checked against the recording and marked [verse] / [chorus].',
        'Instrumentals ticked, songs with vocals unticked.',
        'A rare trigger word is set.',
        'About 22 GB of VRAM free, generation stopped.',
      ],
    },
  ],
};

const zh: Guide = {
  title: 'LoRA 训练指南',
  intro: 'LoRA 是模型的一个小附加件，它学习像你的歌曲那样写歌：结构、人声、编曲。在 MiniMax 中学习的是规划器——模型中负责创作歌曲的部分。可以拖动标题栏移动窗口，拖动右下角调整大小。',
  expandAll: '全部展开',
  collapseAll: '全部收起',
  close: '关闭',
  resize: '拖动以调整大小',
  sections: [
    {
      title: "操作流程",
      steps: [
        "把同一艺人（最好同一张专辑）的歌曲文件夹拖到“训练”页面，或用按钮选择文件夹和文件。工作室会以文件夹名自动建立数据集。支持 WAV、MP3、FLAC、OGG、M4A；带 .cue 的整轨专辑会切成单曲，旁边 .txt 或 .lrc 中的歌词直接采用。训练器不接受超过 6 分钟的歌曲。",
        "第 1 步“歌曲”：准备会自动开始。工作室先从歌词库取词，只对找不到的歌分离人声并识别，聆听每首歌，并用测得的速度和调性写出三部分描述。每首歌都有自己的状态，整体进度在上方。",
        "每首歌都会记住进行到哪一步。如果准备过程中关闭工作室或它崩溃，下次启动时会从同一处自动继续，已完成的不会重做。出错的歌有“重试”按钮，未完成的有“完成这首”按钮，只处理这一首。",
        "检查结果：点击一首歌，它会展开播放器、描述和歌词。按需修改；“重新描述”和“重新查找”只重做这一首。",
        "第 2 步“训练”：LoRA 名称、触发词（工作室会用数据集名称生成一个少见的词，可以修改或清空）、就绪检查和“训练”按钮。训练文件（约 11.3 GB，只需一次）就在这里下载。需要 NVIDIA RTX 30 系列或更新、22 GB 显存的显卡（RTX 3090、4090、5090）。",
        "不必等待：准备进行时勾选“所有歌曲就绪后自动开始训练”。",
        "第 3 步“结果”：试听检查点，在最好的那个下面点“加入 LoRA”，它会出现在 LoRA 页面。训练期间无法生成、使用助手、卡拉OK和分轨。",
      ],
    },
    {
      title: '选择哪些歌曲',
      list: [
        '同一位艺人，最好是同一张专辑或同一时期。这个配方的目标是“克隆一张专辑”：相似度比通用性更重要。',
        '通常 5 到 20 首。训练器作者没有发现 10 首和 20 首之间有可靠差别；歌词密集或风格混杂的专辑更难学。',
        '录音质量一致：录音室版本，不要现场噪音、片头或截断的片段。',
        '完整的歌曲，有开头也有结尾：模型会学习歌曲的结构，包括如何结束。',
      ],
    },
    {
      title: '描述——写什么',
      text: [
        '每首歌各写一份英文描述，约 250–450 词，分三部分：Global Metadata、Vocal Details、Arrangement。',
        '不要整张专辑共用一份描述：训练器作者这样做时歌曲不再正常结束（6 首中 0 首自然结束，而逐首描述时是 6 首中 4 首）。',
        '不要引用或转述歌词，也不要写歌名——歌词在单独的字段里。',
        '只有确实知道时才写准确的 BPM 和调性（来自分析工具或曲库）；否则写范围：“bpm is around 118-124”。',
        '不用写触发词——工作室会自动把它放在 Global Metadata 的最前面。',
      ],
      examples: [{ label: '框架', body: CAPTION_SKELETON }],
    },
    {
      title: "自动描述歌曲",
      text: [
        "可选的包（约 10.5 GB），在“歌曲”步骤的“自动描述歌曲”卡片中下载一次。有了它，每首歌的描述由能听到歌曲的模型来写。",
        "每个模型对整个数据集只加载一次，并在其阶段结束后释放：先查歌词库，再对找不到的歌分离人声并识别，然后聆听，最后是用于歌词的助手。显卡上始终只有一个模型。因此五十首歌的数据集并不需要单首的五十倍时间。",
      ],
      list: [
        "MOSS-Music-8B 聆听歌曲，直接写出 MiniMax 格式的描述：Global Metadata、Vocal Details、Arrangement。HOT-Step 训练器作者也这样做描述。",
        "Beat This! 在录音中找出节拍并据此计算速度，S-KEY（Deezer）判断调性。MOSS 写入描述的数字总是替换为测得的数值。",
        "MOSS 约占 12 GB 显存；在 RTX 4090 上每首歌约 6–7 秒。一切都在你的电脑上运行，不会发送到任何地方。",
        "没有这个包时，歌曲有“描述”按钮，“⋯”菜单中有“全部描述”：助手根据字段中已有的内容、标题和歌词写描述。它听不到歌曲，所以请先简单写下听到的内容，并用耳朵检查结果。",
      ],
    },
    {
      title: "歌词",
      list: [
        "准确写出唱的词，不要和弦、链接或注释。网站上的歌词务必对照录音核对。",
        "标注段落：[verse]、[chorus]、[bridge]、[outro]，每段另起一行。",
        "与音频同名的 .txt 或 .lrc 会在添加时读取；.lrc 中的时间标记会被去掉。",
        "没有歌词时，工作室会按艺人、标题和时长在公开歌词库（LRCLIB、QQ 音乐、酷狗）中查找（艺人和标题取自文件标签，或文件名和文件夹）。只有所有歌词库都不认识这首歌时，才会分离人声并用 Whisper 识别，准确度明显更低。歌词上方会显示来源。请务必检查结果。",
        "如果识别器没听到歌词，这首歌会被标为纯音乐。如果不对，取消“纯音乐”并点“重新查找”。",
      ],
    },
    {
      title: "工作室自动完成的事",
      list: [
        "把所有文件转为 WAV，并为训练器准备声音编码。",
        "把触发词放在每条描述 Global Metadata 的最前面；生成时选择这个 LoRA，也会放进描述。",
        "没有标题的描述会被包进 Global Metadata，所以 YuE2 Studio 的一行风格也能用。",
        "每 100 步保存一次检查点。",
      ],
    },
    {
      title: "需要你自己做的事",
      list: [
        "挑选歌曲并检查音质。",
        "检查工作室写出的描述和歌词：模型和识别都会出错。没有自动描述包时，需要自己或用“描述”为每首歌写描述。",
        "凭耳朵选择检查点——不能按误差曲线选。",
      ],
    },
    {
      title: '训练设置',
      text: ['默认值是 HOT-Step 训练器作者的“Balanced”配方。没有理由时不要改动。'],
      list: [
        '600 步。作者还有快速版 300 步和充分版 900 步。',
        '也可以不按步数而按轮次：一轮就是把数据集中的每首歌都过一遍，步数由工作室计算。',
        'HOT-PiZZA，秩 128，alpha 128，秩丢弃 0.1，AdamW，学习率 8e-5。不要提高学习率：作者试过的所有加倍学习率版本听起来都更差，有一个甚至规划出完全没有人声的歌。',
        '窗口 1536 帧——一次约 61 秒的歌曲。更大的窗口能更好地学习歌曲结构和结尾，但需要更多显存；9000 表示整首歌，需要 32 GB 的显卡。',
        '每 100 步保存一次。',
      ],
    },
    {
      title: '选择检查点与生成',
      list: [
        '凭耳朵选择：用不同检查点生成同一首歌对比。训练器作者发现，损失最低的检查点比听起来正确的那个早 1–8 倍。',
        '步数越多越相似，但连贯性越差：人声可能开始磕绊。',
        '在想要的步数下点击“加入 LoRA”——它会以“训练 · 步数”的名字出现在 LoRA 页面。',
        '在“创建”页选择这个 LoRA 时，触发词会自动加入。强度默认为 1；对于“练过头”的检查点，从 0.5–0.75 开始。',
        '即使只想要短曲，也写整首歌的歌词。按艺人风格写长短不一的句子，比整齐的四行诗效果更好。',
      ],
    },
    {
      title: '出现问题时',
      list: [
        '歌曲过早结束、前奏嗡嗡作响、人声散架——LoRA 训练过头了：换更早的检查点或降低强度。训练损失低于约 1 是死记硬背的迹象。',
        'LoRA 几乎没有变化——换更晚的检查点，检查描述，增加歌曲。',
        '歌曲不再正常结束——检查是否每首歌都有自己的描述，而不是共用一份。',
        '显存不足——关闭占用显卡的程序，或减小帧窗口。',
      ],
    },
    {
      title: '开始训练前的检查清单',
      checklist: [
        '同一艺人或专辑的歌曲 5–20 首，质量良好，完整。',
        '没有重复或残缺片段，没有超过 6 分钟的歌。',
        '每首歌都有自己的三段式描述，并已对照歌曲检查。',
        '描述中没有歌词引用和歌名。',
        '歌词已对照录音核对，并标注 [verse] / [chorus]。',
        '纯音乐已勾选，有人声的歌未勾选。',
        '已设置罕见的触发词。',
        '约 22 GB 显存空闲，已停止生成。',
      ],
    },
  ],
};

const ja: Guide = {
  title: 'LoRA 学習ガイド',
  intro: 'LoRA はモデルへの小さな追加で、あなたの曲のような曲の作り方（構成、ボーカル、アレンジ）を学びます。MiniMax で学習するのはプランナー、つまり曲を作曲する部分です。タイトルバーをドラッグして移動、右下の角でサイズを変えられます。',
  expandAll: 'すべて開く',
  collapseAll: 'すべて閉じる',
  close: '閉じる',
  resize: 'ドラッグでサイズ変更',
  sections: [
    {
      title: "作業の流れ",
      steps: [
        "同じアーティスト（できれば同じアルバム）の曲のフォルダーを「学習」ページにドロップするか、ボタンでフォルダーやファイルを選びます。スタジオがフォルダー名でデータセットを作ります。WAV、MP3、FLAC、OGG、M4A に対応。.cue 付きの一枚ファイルのアルバムは曲ごとに分割され、横の .txt や .lrc の歌詞はそのまま使われます。6 分を超える曲はトレーナーが受け付けません。",
        "ステップ 1「曲」：準備は自動で始まります。歌詞はまずデータベースから取り、見つからない曲だけボーカルを分離して認識し、各曲を聴いて、測定したテンポとキー付きで 3 部構成のキャプションを書きます。曲ごとに状態が表示され、全体の進行は上に出ます。",
        "各曲はどこまで進んだかを覚えています。準備中にスタジオを閉じたり落ちたりしても、次に起動すると同じところから自動で続き、済んだ作業は繰り返しません。失敗した曲には「再試行」、途中の曲には「この曲を仕上げる」ボタンがあり、その曲だけを仕上げます。",
        "結果を確認：曲をクリックすると、プレーヤー、キャプション、歌詞が開きます。必要に応じて修正し、「もう一度説明」「探し直す」はその曲だけをやり直します。",
        "ステップ 2「学習」：LoRA の名前、トリガーワード（データセット名からスタジオが珍しい単語を作ります。変更も削除もできます）、準備状況の確認と「学習」ボタン。学習ファイル（約 11.3 GB、一度だけ）もここでダウンロードします。22 GB のビデオメモリを持つ NVIDIA RTX 30 シリーズ以降（RTX 3090、4090、5090）が必要です。",
        "待つ必要はありません：準備中に「全曲の準備ができたら自動で学習を開始」にチェックを入れてください。",
        "ステップ 3「結果」：チェックポイントを聴き比べ、一番良いものの下の「LoRA へ」を押すと LoRA ページに表示されます。学習中は生成、アシスタント、カラオケ、ステム分離は使えません。",
      ],
    },
    {
      title: 'どの曲を使うか',
      list: [
        '一人のアーティスト、できれば一枚のアルバムか一つの時期。このレシピは「アルバムの複製」向けで、幅より似ていることを重視します。',
        '通常 5〜20 曲。学習器の作者は 10 曲と 20 曲で確かな差を見ていません。歌詞の多いアルバムや作風がばらばらなアルバムは学びにくいです。',
        '録音品質をそろえる：スタジオ版で、ライブの雑音、ジングル、途切れた断片は避けます。',
        '始まりと終わりのある曲をまるごと：モデルは曲の構成、終わり方まで学びます。',
      ],
    },
    {
      title: 'キャプション — 何を書くか',
      text: [
        '曲ごとに自分の説明を英語で、約 250〜450 語、三つの部分に分けて：Global Metadata、Vocal Details、Arrangement。',
        'アルバム全体で一つの説明にしないでください。学習器の作者の場合、曲がきちんと終わらなくなりました（自然な終わりが 6 曲中 0 曲、曲ごとの説明では 6 曲中 4 曲）。',
        '歌詞を引用・言い換えせず、曲名も書きません。歌詞は別の欄に入れます。',
        '正確な BPM とキーは、分かっているとき（解析ツールや曲データベース）だけ書き、そうでなければ範囲で：「bpm is around 118-124」。',
        'トリガーワードは書かないでください。スタジオが Global Metadata の先頭に自動で入れます。',
      ],
      examples: [{ label: 'ひな形', body: CAPTION_SKELETON }],
    },
    {
      title: "曲の自動説明",
      text: [
        "任意のパック（約 10.5 GB）で、「曲」ステップの「曲の自動説明」カードから一度だけダウンロードします。これがあると、各曲のキャプションを曲を聴けるモデルが書きます。",
        "各モデルはデータセット全体で一度だけ読み込まれ、その段階が終わると解放されます：歌詞データベース、見つからない曲のボーカル分離と認識、聴き取り、歌詞用のアシスタントの順です。GPU に載るモデルは常に一つです。だから 50 曲のデータセットでも 1 曲の 50 倍はかかりません。",
      ],
      list: [
        "MOSS-Music-8B は曲を聴いて、MiniMax 形式のキャプションをそのまま書きます：Global Metadata、Vocal Details、Arrangement。HOT-Step トレーナーの作者も同じ方法でキャプションを作っています。",
        "Beat This! が録音の拍を見つけてテンポを計算し、S-KEY（Deezer）がキーを判定します。MOSS がキャプションに書いた数値は常に測定値に置き換えます。",
        "MOSS は約 12 GB のビデオメモリを使い、RTX 4090 なら 1 曲 6〜7 秒です。すべてあなたのコンピューターで動き、どこにも送信されません。",
        "パックがないときは、曲に「説明を書く」ボタン、「⋯」メニューに「すべて説明」があります。アシスタントは欄にすでにある内容、タイトル、歌詞からキャプションを書きます。曲は聴けないので、まず聞こえるものを短く書き、結果を耳で確認してください。",
      ],
    },
    {
      title: "歌詞",
      list: [
        "実際に歌われている言葉だけを、コード、リンク、注釈なしで。サイトの歌詞は必ず録音と照合してください。",
        "パートを示します：[verse]、[chorus]、[bridge]、[outro]。各パートは改行して始めます。",
        "音声と同じ名前の .txt や .lrc は追加時に読み込まれ、.lrc のタイムスタンプは取り除かれます。",
        "歌詞がないときは、スタジオがアーティスト、タイトル、長さで公開歌詞データベース（LRCLIB、QQ Music、Kugou）を検索します（アーティストとタイトルはファイルのタグ、または名前とフォルダーから）。どのデータベースにもない曲だけ、ボーカルを分離して Whisper で認識します。精度はかなり下がります。歌詞の上に出どころが表示されます。結果は必ず確認してください。",
        "認識器が歌詞を聞き取れなかった曲はインストゥルメンタルになります。誤りなら「インストゥルメンタル」を外して「探し直す」を押してください。",
      ],
    },
    {
      title: "スタジオが自動でやること",
      list: [
        "すべてを WAV に変換し、トレーナー用の音声コードを用意します。",
        "各キャプションの Global Metadata の先頭にトリガーワードを置き、生成時にこの LoRA を選ぶとキャプションにも入れます。",
        "見出しのないキャプションは Global Metadata で包むので、YuE2 Studio の一行も使えます。",
        "100 ステップごとにチェックポイントを保存します。",
      ],
    },
    {
      title: "自分でやること",
      list: [
        "曲を選び、音質を確認する。",
        "スタジオが書いたキャプションと歌詞を確認する。モデルも認識も間違えます。自動説明パックがなければ、各曲のキャプションを自分で、または「説明を書く」で用意します。",
        "チェックポイントを耳で選ぶ。誤差のグラフでは選べません。",
      ],
    },
    {
      title: '学習の設定',
      text: ['既定値は HOT-Step 学習器の作者の「Balanced」レシピです。理由がなければ変えないでください。'],
      list: [
        '600 ステップ。作者には速い 300 と念入りな 900 もあります。',
        'ステップの代わりにエポックも選べます。1 エポックはデータセットの全曲を一巡すること、ステップ数はスタジオが計算します。',
        'HOT-PiZZA、ランク 128、alpha 128、ランクドロップアウト 0.1、AdamW、学習率 8e-5。学習率は上げないでください：作者が試した倍の学習率はどれも音が悪く、一つはボーカルのない曲を作りました。',
        'ウィンドウ 1536 フレーム — 一度に約 61 秒。大きいほど曲の構成と終わり方をよく学びますが、メモリを多く使います。9000 は曲全体で、32 GB のカードが必要です。',
        '100 ステップごとに保存。',
      ],
    },
    {
      title: 'チェックポイントの選び方と生成',
      list: [
        '耳で選びます：同じ曲を別々のチェックポイントで生成して比べます。学習器の作者の場合、損失が一番良いチェックポイントは、正しく聴こえるものより 1〜8 倍早い時点でした。',
        'ステップが多いほど似ますが、まとまりは落ち、ボーカルがつまずき始めることがあります。',
        '選んだステップの下で「LoRA に追加」を押すと、「実行 · ステップ」という名前で LoRA ページに出ます。',
        '「作成」ページでこの LoRA を選ぶと、トリガーは自動で加わります。強さの既定は 1。学習しすぎのチェックポイントは 0.5〜0.75 から始めます。',
        '短い曲が欲しくても、曲全体の歌詞を書いてください。アーティストらしい長さの不ぞろいな行の方が、きれいな四行詩よりうまくいきます。',
      ],
    },
    {
      title: 'うまくいかないとき',
      list: [
        '曲が早く終わる、イントロがうなり続ける、ボーカルが崩れる — 学習しすぎです。前のチェックポイントにするか強さを下げます。学習損失が約 1 を下回るのは暗記のサインです。',
        'LoRA でほとんど変わらない — 後のチェックポイントにし、説明を確認し、曲を増やします。',
        '曲がきちんと終わらなくなった — 全曲共通の説明ではなく、曲ごとの説明になっているか確認します。',
        'VRAM が足りない — カードを使っているものを閉じるか、フレームウィンドウを小さくします。',
      ],
    },
    {
      title: '開始前のチェックリスト',
      checklist: [
        '同じアーティストかアルバムの曲が 5〜20 曲、品質良好、まるごと。',
        '重複や断片がなく、6 分を超える曲がない。',
        '各曲に三部構成の自分の説明があり、曲と照らし合わせて確認した。',
        '説明に歌詞の引用や曲名がない。',
        '歌詞を録音と照合し、[verse] / [chorus] を付けた。',
        'インストにはチェック、ボーカル曲はチェックなし。',
        '珍しいトリガーワードを設定した。',
        'VRAM が約 22 GB 空いていて、生成は止めてある。',
      ],
    },
  ],
};

const ko: Guide = {
  title: 'LoRA 학습 안내',
  intro: 'LoRA는 모델에 붙는 작은 추가 파일로, 당신의 곡처럼 곡을 쓰는 법(구성, 보컬, 편곡)을 배웁니다. MiniMax에서 배우는 것은 플래너, 즉 곡을 작곡하는 부분입니다. 제목 표시줄을 끌어 옮기고, 오른쪽 아래 모서리로 크기를 바꿀 수 있습니다.',
  expandAll: '모두 펼치기',
  collapseAll: '모두 접기',
  close: '닫기',
  resize: '끌어서 크기 조절',
  sections: [
    {
      title: "작업 순서",
      steps: [
        "한 아티스트(가능하면 한 앨범)의 노래 폴더를 \"학습\" 페이지에 끌어다 놓거나 버튼으로 폴더와 파일을 고르세요. 스튜디오가 폴더 이름으로 데이터셋을 만듭니다. WAV, MP3, FLAC, OGG, M4A를 지원하고, .cue가 있는 한 파일짜리 앨범은 곡별로 나뉘며, 옆의 .txt나 .lrc 가사는 그대로 사용합니다. 6분이 넘는 곡은 트레이너가 받지 않습니다.",
        "1단계 \"곡\": 준비가 저절로 시작됩니다. 가사는 먼저 데이터베이스에서 가져오고, 없는 곡만 보컬을 분리해 인식하며, 각 곡을 듣고, 측정한 템포와 키로 세 부분짜리 캡션을 씁니다. 곡마다 상태가 보이고, 전체 진행은 위에 있습니다.",
        "곡마다 어디까지 했는지 기억합니다. 준비 중에 스튜디오를 닫거나 멈추면 다음 실행 때 같은 곳에서 저절로 이어지고, 끝난 작업은 다시 하지 않습니다. 실패한 곡에는 '다시 시도', 덜 된 곡에는 '이 곡 마무리' 버튼이 있어 그 곡만 마무리합니다.",
        "결과를 확인하세요: 곡을 누르면 플레이어, 캡션, 가사가 펼쳐집니다. 필요한 것을 고치세요. \"다시 설명\"과 \"다시 찾기\"은 그 곡만 다시 합니다.",
        "2단계 \"학습\": LoRA 이름, 트리거 단어(데이터셋 이름으로 스튜디오가 드문 단어를 만들어 줍니다. 바꾸거나 지울 수 있습니다), 준비 확인과 \"학습\" 버튼. 학습 파일(약 11.3GB, 한 번)도 여기서 내려받습니다. 비디오 메모리 22GB의 NVIDIA RTX 30 시리즈 이상(RTX 3090, 4090, 5090)이 필요합니다.",
        "기다릴 필요 없습니다: 준비 중에 \"모든 곡이 준비되면 자동으로 학습 시작\"을 체크하세요.",
        "3단계 \"결과\": 체크포인트를 들어 보고 가장 좋은 것 아래의 \"LoRA로\"를 누르면 LoRA 페이지에 나타납니다. 학습 중에는 생성, 어시스턴트, 가라오케, 스템 분리를 쓸 수 없습니다.",
      ],
    },
    {
      title: '어떤 곡을 쓸까',
      list: [
        '한 아티스트, 가능하면 한 앨범이나 한 시기. 이 레시피는 「앨범 복제」에 맞춰져 있어 폭보다 닮음이 중요합니다.',
        '보통 5~20곡. 학습기 제작자는 10곡과 20곡 사이에 뚜렷한 차이를 보지 못했습니다. 가사가 빽빽하거나 스타일이 섞인 앨범은 배우기 더 어렵습니다.',
        '녹음 품질을 고르게: 스튜디오 버전, 라이브 잡음·징글·잘린 조각은 제외합니다.',
        '시작과 끝이 있는 곡을 통째로: 모델은 곡의 구성, 끝나는 방식까지 배웁니다.',
      ],
    },
    {
      title: '캡션 — 무엇을 쓸까',
      text: [
        '곡마다 자기 설명을 영어로, 약 250~450 단어, 세 부분으로: Global Metadata, Vocal Details, Arrangement.',
        '앨범 전체에 설명 하나를 쓰지 마세요. 학습기 제작자의 경우 곡이 제대로 끝나지 않게 되었습니다(자연스러운 끝 6곡 중 0곡, 곡별 설명일 때는 6곡 중 4곡).',
        '가사를 인용하거나 바꿔 쓰지 말고 곡 제목도 쓰지 마세요. 가사는 따로 들어갑니다.',
        '정확한 BPM과 조성은 알 때만(분석 도구나 곡 데이터베이스) 쓰고, 아니면 범위로: 「bpm is around 118-124」.',
        '트리거 단어는 쓰지 마세요. 스튜디오가 Global Metadata 맨 앞에 알아서 넣습니다.',
      ],
      examples: [{ label: '뼈대', body: CAPTION_SKELETON }],
    },
    {
      title: "곡 자동 설명",
      text: [
        "선택 패키지(약 10.5GB)로, \"곡\" 단계의 \"곡 자동 설명\" 카드에서 한 번 내려받습니다. 이것이 있으면 각 곡의 캡션을 곡을 들을 수 있는 모델이 씁니다.",
        "각 모델은 데이터셋 전체에 한 번만 로드되고, 그 단계가 끝나면 해제됩니다: 가사 데이터베이스, 없는 곡의 보컬 분리와 인식, 듣기, 가사용 어시스턴트 순서입니다. 그래픽 카드에는 항상 모델 하나만 올라갑니다. 그래서 50곡짜리 데이터셋도 한 곡의 50배가 걸리지 않습니다.",
      ],
      list: [
        "MOSS-Music-8B는 곡을 듣고 MiniMax 형식의 캡션을 바로 씁니다: Global Metadata, Vocal Details, Arrangement. HOT-Step 트레이너 작성자도 같은 방법으로 캡션을 만듭니다.",
        "Beat This!가 녹음의 박을 찾아 템포를 계산하고, S-KEY(Deezer)가 키를 판별합니다. MOSS가 캡션에 쓴 숫자는 항상 측정값으로 바꿉니다.",
        "MOSS는 약 12GB의 비디오 메모리를 쓰고, RTX 4090에서 한 곡에 6–7초가 걸립니다. 모든 것이 여러분의 컴퓨터에서 돌아가며 어디에도 보내지 않습니다.",
        "패키지가 없으면 곡에 \"설명 쓰기\" 버튼이, \"⋯\" 메뉴에 \"모두 설명\"이 있습니다. 어시스턴트는 칸에 이미 있는 내용, 제목, 가사로 캡션을 씁니다. 곡을 듣지 못하므로 먼저 들리는 것을 짧게 적고 결과를 귀로 확인하세요.",
      ],
    },
    {
      title: "가사",
      list: [
        "실제로 부르는 가사만, 코드나 링크, 메모 없이. 사이트의 가사는 반드시 녹음과 대조하세요.",
        "파트를 표시하세요: [verse], [chorus], [bridge], [outro]. 각 파트는 새 줄에서 시작합니다.",
        "오디오와 이름이 같은 .txt나 .lrc는 추가할 때 읽히고, .lrc의 타임스탬프는 지워집니다.",
        "가사가 없으면 스튜디오가 아티스트, 제목, 길이로 공개 가사 데이터베이스(LRCLIB, QQ Music, Kugou)에서 찾습니다(아티스트와 제목은 파일 태그나 이름과 폴더에서 가져옵니다). 어느 데이터베이스에도 없는 곡만 보컬을 분리해 Whisper로 인식하며, 정확도는 훨씬 낮습니다. 가사 위에 출처가 표시됩니다. 결과는 항상 확인하세요.",
        "인식기가 가사를 듣지 못한 곡은 연주곡으로 표시됩니다. 틀렸다면 \"연주곡\"을 끄고 \"다시 찾기\"을 누르세요.",
      ],
    },
    {
      title: "스튜디오가 알아서 하는 일",
      list: [
        "모든 것을 WAV로 바꾸고 트레이너용 소리 코드를 준비합니다.",
        "각 캡션의 Global Metadata 맨 앞에 트리거 단어를 넣고, 생성할 때 이 LoRA를 고르면 캡션에도 넣습니다.",
        "제목 없는 캡션은 Global Metadata로 감싸므로 YuE2 Studio의 한 줄도 쓸 수 있습니다.",
        "100단계마다 체크포인트를 저장합니다.",
      ],
    },
    {
      title: "직접 해야 하는 일",
      list: [
        "곡을 고르고 음질을 확인하기.",
        "스튜디오가 쓴 캡션과 가사를 확인하기. 모델도 인식도 틀립니다. 자동 설명 패키지가 없으면 각 곡의 캡션을 직접 또는 \"설명 쓰기\"로 준비하기.",
        "체크포인트를 귀로 고르기. 손실 그래프로는 고를 수 없습니다.",
      ],
    },
    {
      title: '학습 설정',
      text: ['기본값은 HOT-Step 학습기 제작자의 「Balanced」 레시피입니다. 이유 없이 바꾸지 마세요.'],
      list: [
        '600 스텝. 제작자에게는 빠른 300과 꼼꼼한 900도 있습니다.',
        '스텝 대신 에포크를 고를 수도 있습니다. 에포크 하나는 데이터셋의 모든 곡을 한 번 도는 것이고, 단계 수는 스튜디오가 계산합니다.',
        'HOT-PiZZA, 랭크 128, alpha 128, 랭크 드롭아웃 0.1, AdamW, 학습률 8e-5. 학습률을 올리지 마세요: 제작자가 시험한 두 배 학습률은 모두 더 나빴고, 하나는 보컬이 전혀 없는 곡을 만들었습니다.',
        '창 1536 프레임 — 한 번에 약 61초. 창이 클수록 곡의 구성과 끝을 더 잘 배우지만 메모리가 더 듭니다. 9000은 곡 전체로 32 GB 카드가 필요합니다.',
        '100 스텝마다 저장.',
      ],
    },
    {
      title: '체크포인트 고르기와 생성',
      list: [
        '귀로 고르세요: 같은 곡을 여러 체크포인트로 생성해 비교합니다. 학습기 제작자의 경우 손실이 가장 좋은 체크포인트는 제대로 들리는 것보다 1~8배 이른 시점이었습니다.',
        '스텝이 많을수록 더 닮지만 짜임새는 떨어지고, 보컬이 더듬기 시작할 수 있습니다.',
        '원하는 스텝 아래의 「LoRA에 추가」를 누르면 「실행 · 스텝」 이름으로 LoRA 페이지에 나타납니다.',
        '「만들기」 페이지에서 이 LoRA를 고르면 트리거가 알아서 들어갑니다. 강도 기본값은 1이고, 과하게 학습된 체크포인트는 0.5~0.75부터 시작하세요.',
        '짧은 곡을 원해도 곡 전체의 가사를 쓰세요. 아티스트다운 길이가 들쭉날쭉한 줄이 깔끔한 4행보다 잘 됩니다.',
      ],
    },
    {
      title: '문제가 있을 때',
      list: [
        '곡이 일찍 끝나거나, 인트로가 웅웅거리거나, 보컬이 무너지면 과학습입니다. 더 이른 체크포인트를 쓰거나 강도를 낮추세요. 학습 손실이 약 1 아래면 암기의 신호입니다.',
        'LoRA가 거의 아무것도 바꾸지 않으면 더 늦은 체크포인트를 쓰고, 설명을 확인하고, 곡을 늘리세요.',
        '곡이 제대로 끝나지 않게 되었다면 모든 곡이 공용 설명이 아니라 자기 설명을 가졌는지 확인하세요.',
        'VRAM이 부족하면 카드를 쓰는 프로그램을 닫거나 프레임 창을 줄이세요.',
      ],
    },
    {
      title: '시작 전 체크리스트',
      checklist: [
        '같은 아티스트나 앨범의 곡 5~20곡, 좋은 품질, 통째로.',
        '중복이나 잘린 조각이 없고, 6분보다 긴 곡이 없음.',
        '모든 곡에 세 부분으로 된 자기 설명이 있고, 곡과 대조해 확인함.',
        '설명에 가사 인용이나 곡 제목이 없음.',
        '가사를 녹음과 대조하고 [verse] / [chorus]를 표시함.',
        '연주곡은 체크, 보컬 곡은 체크 해제.',
        '드문 트리거 단어를 정함.',
        'VRAM 약 22 GB 여유, 생성은 멈춤.',
      ],
    },
  ],
};

export const trainingGuide: Record<Language, Guide> = { en, ru, zh, ja, ko };
