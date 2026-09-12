import assert from 'node:assert/strict';
import test from 'node:test';
import {
  apply,
  approve,
  deny,
  errorCodes,
  extend,
  getAccessRequest,
  httpRoutes,
  joinUrl,
  languageIds,
  languages,
  listAccessRequests,
  listSessions,
  login,
  logout,
  problems,
  revoke,
  setLanguageEnabled,
} from '../generated/tinker-http.js';

test('module lists apply, approve, languages, problems, login', () => {
  for (const fn of [apply, approve, languages, problems, login]) {
    assert.equal(typeof fn, 'function');
  }
  const names = httpRoutes.map((r) => r.name);
  for (const name of ['apply', 'approve', 'languages', 'problems', 'login']) {
    assert.ok(names.includes(name), name);
  }
  assert.ok(errorCodes.includes('full'));
  assert.ok(languageIds.includes('python'));
});

test('joinUrl strips trailing slashes with a linear scan', () => {
  assert.equal(joinUrl('http://x///', '/v1/login'), 'http://x/v1/login');
  assert.equal(joinUrl('http://x', 'v1/login'), 'http://x/v1/login');
  assert.equal(joinUrl('', '/v1/login'), '/v1/login');
});

test('fetch helpers send JSON and credentials', async () => {
  const calls = [];
  const previous = globalThis.fetch;
  globalThis.fetch = (url, init) => {
    calls.push({ url, init });
    return Promise.resolve({ ok: true, url, init });
  };
  try {
    await apply('http://pub///', { display_name: 'Ada' });
    await getAccessRequest('http://pub', 'abc', 'tok');
    await languages('http://pub');
    await problems('http://pub', { headers: { 'x-test': '1' } });
    await login('http://adm', { password: 'x' });
    await logout('http://adm');
    await listAccessRequests('http://adm');
    await approve('http://adm', 'abc', { ttl_seconds: 60 });
    await deny('http://adm', 'abc');
    await listSessions('http://adm');
    await revoke('http://adm', 'sid');
    await extend('http://adm', 'sid', { ttl_seconds: 10 });
    await setLanguageEnabled('http://adm', 'python', { enabled: true });
  } finally {
    globalThis.fetch = previous;
  }

  assert.equal(calls.length, 13);
  assert.equal(calls[0].url, 'http://pub/v1/access-requests');
  assert.equal(calls[0].init.method, 'POST');
  assert.equal(JSON.parse(calls[0].init.body).display_name, 'Ada');
  assert.equal(calls[1].url, 'http://pub/v1/access-requests/abc');
  assert.equal(calls[1].init.headers.authorization, 'Bearer tok');
  assert.equal(calls[2].url, 'http://pub/v1/languages');
  assert.equal(calls[3].init.headers['x-test'], '1');
  assert.equal(calls[4].url, 'http://adm/v1/login');
  assert.equal(calls[4].init.credentials, 'include');
  assert.equal(calls[7].url, 'http://adm/v1/access-requests/abc/approve');
  assert.equal(JSON.parse(calls[7].init.body).ttl_seconds, 60);
  assert.equal(calls[12].url, 'http://adm/v1/languages/python/enabled');
});
