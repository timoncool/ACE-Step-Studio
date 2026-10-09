import { describe, expect, it } from 'vitest';
import { scriptLanguage } from './lyricsLanguage';

describe('scriptLanguage', () => {
  it('names the language of a script and leaves Latin alone', () => {
    expect(scriptLanguage('[Verse]\nПыль дороги на сапогах\nИ звезда над головой')).toBe('ru');
    expect(scriptLanguage('[Verse]\nЇжак іде додому через ліс')).toBe('uk');
    expect(scriptLanguage('[Chorus]\n사랑해요 오늘 밤 함께 노래해')).toBe('ko');
    expect(scriptLanguage('[Verse]\n夜空に光る星を見上げて')).toBe('ja');
    expect(scriptLanguage('[Verse]\n我们一起唱歌到天亮吧朋友')).toBe('zh');
    expect(scriptLanguage('[Verse]\nWalking down the river in the morning light')).toBeNull();
    expect(scriptLanguage('[Intro]\n[Chorus]')).toBeNull();
  });
});
