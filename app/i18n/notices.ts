/** Strings of the hub notices and the telemetry choice; engine-neutral, the same in every studio of the family. */

const en = {
  hubNoticesRegion: 'Notices',
  hubClose: 'Close',
  hubAd: 'Ad',
  hubTelemetryTitle: 'Anonymous statistics',
  hubTelemetryCheckbox: 'Send anonymous usage statistics',
  hubTelemetryWhy: 'How many people use the studio and what for helps decide what to work on next. Nothing personal: no lyrics, prompts, audio or file names — only counts, the version and the graphics card class.',
  hubTelemetryWhat: 'What is sent',
  hubTelemetryEnabled: 'Statistics are on',
  hubTelemetryDisabled: 'Statistics are off',
  hubTelemetryByEnv: 'Turned off by DO_NOT_TRACK or STUDIO_TELEMETRY in the environment.',
  hubTelemetryReset: 'New install id',
  hubTelemetryNothing: 'Nothing is waiting to be sent today.',
  hubFeedSource: 'News from',
  hubFeedNever: 'News have not been fetched yet: the studio shows the news of its release.',
  hubFeedRefresh: 'Check now',
  hubSettingsHint: "Anonymous statistics and news from the studio's author",
};

type Strings = typeof en;

const ru: Strings = {
  hubNoticesRegion: 'Уведомления',
  hubClose: 'Закрыть',
  hubAd: 'Реклама',
  hubTelemetryTitle: 'Анонимная статистика',
  hubTelemetryCheckbox: 'Отправлять анонимную статистику использования',
  hubTelemetryWhy: 'Сколько людей пользуется студией и для чего — так понятно, что делать дальше. Ничего личного: ни текстов, ни промптов, ни звука, ни имён файлов — только счётчики, версия и класс видеокарты.',
  hubTelemetryWhat: 'Что отправляется',
  hubTelemetryEnabled: 'Статистика включена',
  hubTelemetryDisabled: 'Статистика выключена',
  hubTelemetryByEnv: 'Выключена переменной DO_NOT_TRACK или STUDIO_TELEMETRY.',
  hubTelemetryReset: 'Новый id установки',
  hubTelemetryNothing: 'Сегодня отправлять пока нечего.',
  hubFeedSource: 'Новости с',
  hubFeedNever: 'Новости с сервера ещё не получены: студия показывает новости своего релиза.',
  hubFeedRefresh: 'Проверить сейчас',
  hubSettingsHint: 'Анонимная статистика и новости от автора студии',
};

const zh: Strings = {
  hubNoticesRegion: '通知',
  hubClose: '关闭',
  hubAd: '广告',
  hubTelemetryTitle: '匿名统计',
  hubTelemetryCheckbox: '发送匿名使用统计',
  hubTelemetryWhy: '有多少人在用、用来做什么，决定接下来做什么。不含任何个人信息：没有歌词、提示词、音频或文件名——只有计数、版本和显卡档次。',
  hubTelemetryWhat: '发送的内容',
  hubTelemetryEnabled: '统计已开启',
  hubTelemetryDisabled: '统计已关闭',
  hubTelemetryByEnv: '已被环境变量 DO_NOT_TRACK 或 STUDIO_TELEMETRY 关闭。',
  hubTelemetryReset: '新的安装 ID',
  hubTelemetryNothing: '今天还没有需要发送的内容。',
  hubFeedSource: '新闻来源',
  hubFeedNever: '尚未获取服务器新闻：工作室显示其版本自带的新闻。',
  hubFeedRefresh: '立即检查',
  hubSettingsHint: '匿名统计与来自作者的新闻',
};

const ja: Strings = {
  hubNoticesRegion: 'お知らせ',
  hubClose: '閉じる',
  hubAd: '広告',
  hubTelemetryTitle: '匿名の統計',
  hubTelemetryCheckbox: '匿名の利用統計を送信する',
  hubTelemetryWhy: '何人がどんな用途で使っているかで、次に取り組むことを決めます。個人的な情報は一切なし：歌詞、プロンプト、音声、ファイル名は送らず、回数、バージョン、GPU のクラスだけです。',
  hubTelemetryWhat: '送信される内容',
  hubTelemetryEnabled: '統計はオンです',
  hubTelemetryDisabled: '統計はオフです',
  hubTelemetryByEnv: '環境変数 DO_NOT_TRACK または STUDIO_TELEMETRY でオフになっています。',
  hubTelemetryReset: '新しいインストール ID',
  hubTelemetryNothing: '今日はまだ送信するものがありません。',
  hubFeedSource: 'ニュースの取得元',
  hubFeedNever: 'サーバーのニュースはまだ取得されていません：リリースに含まれるニュースを表示しています。',
  hubFeedRefresh: '今すぐ確認',
  hubSettingsHint: '匿名の統計と作者からのニュース',
};

const ko: Strings = {
  hubNoticesRegion: '알림',
  hubClose: '닫기',
  hubAd: '광고',
  hubTelemetryTitle: '익명 통계',
  hubTelemetryCheckbox: '익명 사용 통계 보내기',
  hubTelemetryWhy: '몇 명이 어떤 용도로 쓰는지로 다음에 할 일을 정합니다. 개인 정보는 없습니다: 가사, 프롬프트, 오디오, 파일 이름은 보내지 않고 횟수, 버전, 그래픽 카드 등급만 보냅니다.',
  hubTelemetryWhat: '보내는 내용',
  hubTelemetryEnabled: '통계가 켜져 있습니다',
  hubTelemetryDisabled: '통계가 꺼져 있습니다',
  hubTelemetryByEnv: '환경 변수 DO_NOT_TRACK 또는 STUDIO_TELEMETRY로 꺼져 있습니다.',
  hubTelemetryReset: '새 설치 ID',
  hubTelemetryNothing: '오늘은 아직 보낼 것이 없습니다.',
  hubFeedSource: '소식 출처',
  hubFeedNever: '아직 서버 소식을 받지 못했습니다: 릴리스에 포함된 소식을 보여 줍니다.',
  hubFeedRefresh: '지금 확인',
  hubSettingsHint: '익명 통계와 제작자의 소식',
};

export const noticeStrings = { en, ru, zh, ja, ko };
