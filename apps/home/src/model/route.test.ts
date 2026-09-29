import { describe, expect, it } from 'vitest';
import {
  HOME_BASE,
  homeHref,
  parseAppPath,
  parseHomeLocation,
  parseInsightsDeepLink,
  stripHomeBase,
} from './route';

describe('stripHomeBase', () => {
  it('maps the home mount root to /', () => {
    expect(stripHomeBase('/home')).toBe('/');
    expect(stripHomeBase('/home/')).toBe('/');
  });

  it('strips the mount from nested paths', () => {
    expect(stripHomeBase('/home/insights')).toBe('/insights');
    expect(stripHomeBase('/home/s/abc')).toBe('/s/abc');
  });

  it('passes root-relative paths through (post-merge)', () => {
    expect(stripHomeBase('/insights')).toBe('/insights');
    expect(stripHomeBase('/s/abc')).toBe('/s/abc');
    expect(stripHomeBase('/')).toBe('/');
  });
});

describe('homeHref', () => {
  it('joins app paths onto the /home mount', () => {
    expect(homeHref('/')).toBe(`${HOME_BASE}/`);
    expect(homeHref('/insights')).toBe(`${HOME_BASE}/insights`);
    expect(homeHref('/s/sess_1')).toBe(`${HOME_BASE}/s/sess_1`);
  });
});

describe('parseAppPath', () => {
  it('recognizes /insights', () => {
    expect(parseAppPath('/insights')).toEqual({
      path: '/insights',
      sessionId: null,
      insights: true,
    });
  });

  it('parses /s/<id> with opaque percent-decoded ids', () => {
    expect(parseAppPath('/s/sess_1a2b')).toEqual({
      path: '/s/sess_1a2b',
      sessionId: 'sess_1a2b',
      insights: false,
    });
    expect(parseAppPath('/s/foo%2Fbar')).toEqual({
      path: '/s/foo%2Fbar',
      sessionId: 'foo/bar',
      insights: false,
    });
  });

  it('returns the deck root for unknown paths', () => {
    expect(parseAppPath('/')).toEqual({ path: '/', sessionId: null, insights: false });
    expect(parseAppPath('/cruise')).toEqual({ path: '/cruise', sessionId: null, insights: false });
  });
});

describe('parseHomeLocation', () => {
  it('parses mounted deep links /home/insights and /home/s/<id>', () => {
    expect(parseHomeLocation('/home/insights')).toEqual({
      path: '/insights',
      sessionId: null,
      insights: true,
    });
    expect(parseHomeLocation('/home/s/abc-123')).toEqual({
      path: '/s/abc-123',
      sessionId: 'abc-123',
      insights: false,
    });
  });

  it('accepts root-relative /insights and /s/<id>', () => {
    expect(parseHomeLocation('/insights').insights).toBe(true);
    expect(parseHomeLocation('/s/xyz').sessionId).toBe('xyz');
  });

  it('treats legacy #insights as insights when the path is the deck root', () => {
    expect(parseHomeLocation('/home/', '#insights')).toEqual({
      path: '/',
      sessionId: null,
      insights: true,
    });
    expect(parseHomeLocation('/home', '#insights').insights).toBe(true);
  });

  it('ignores #insights on a /s/<id> deep link', () => {
    const parsed = parseHomeLocation('/home/s/abc', '#insights');
    expect(parsed).toEqual({
      path: '/s/abc',
      sessionId: 'abc',
      insights: false,
    });
  });
});

describe('parseInsightsDeepLink (legacy)', () => {
  it('matches #insights only', () => {
    expect(parseInsightsDeepLink('#insights')).toBe(true);
    expect(parseInsightsDeepLink('#other')).toBe(false);
    expect(parseInsightsDeepLink('')).toBe(false);
    expect(parseInsightsDeepLink(null)).toBe(false);
  });
});
