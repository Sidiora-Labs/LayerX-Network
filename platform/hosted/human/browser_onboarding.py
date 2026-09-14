import argparse
import hashlib
import http.cookies
import json
import os
from pathlib import Path
import re
import selectors
import ssl
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

from provision import fields, protected_bytes, protected_json, require, strict_pairs, write_json

MAX_FRAME = 1048576


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise ValueError('Human request redirect refused')


class Api:
    def __init__(self, url, ca, origin):
        endpoint = urllib.parse.urlsplit(url)
        require(endpoint.scheme == 'https' and endpoint.hostname and not endpoint.username
                and not endpoint.password and not endpoint.query and not endpoint.fragment
                and endpoint.path in ('', '/'), 'Human journey', 'pinned HTTPS endpoint')
        self.url, self.origin, self.cookies = url.rstrip('/'), origin, {}
        self.opener = urllib.request.build_opener(NoRedirect(), urllib.request.HTTPSHandler(
            context=ssl.create_default_context(cafile=ca)))

    def call(self, method, path, body=None, action=None, authenticated=True):
        headers = {'Accept': 'application/json', 'Content-Type': 'application/json', 'Origin': self.origin}
        if action is not None:
            headers['Idempotency-Key'] = action
        if authenticated and self.cookies:
            headers['Cookie'] = '; '.join(name + '=' + value for name, value in self.cookies.items())
            if method != 'GET':
                headers['X-CSRF-Token'] = self.cookies['__Host-layerx_csrf']
        request = urllib.request.Request(self.url + path, method=method, headers=headers,
            data=None if body is None else json.dumps(body, allow_nan=False, separators=(',', ':')).encode())
        try:
            response = self.opener.open(request, timeout=30)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            raw = response.read(MAX_FRAME + 1)
            require(len(raw) <= MAX_FRAME, 'Human journey', 'response size')
            value = json.loads(raw, object_pairs_hook=strict_pairs)
            received = response.headers.get_all('Set-Cookie', [])
            status = response.status
        if received:
            require(status == 201 and path == '/v1/sessions', 'Human journey', 'session cookie origin')
            cookies = {}
            for header in received:
                cookie = http.cookies.SimpleCookie()
                cookie.load(header)
                require(len(cookie) == 1, 'Human journey', 'one cookie per header')
                for name, morsel in cookie.items():
                    require(name in ('__Host-layerx_access', '__Host-layerx_refresh', '__Host-layerx_csrf')
                            and name not in cookies and morsel['secure'] and morsel['path'] == '/'
                            and not morsel['domain'] and morsel['samesite'].lower() == 'strict'
                            and morsel.value and not any(ord(c) < 33 or ord(c) > 126 for c in morsel.value),
                            'Human journey', 'protected session cookie')
                    if name != '__Host-layerx_csrf':
                        require(morsel['httponly'], 'Human journey', 'HTTP-only session token')
                    cookies[name] = morsel.value
            require(len(cookies) == 3, 'Human journey', 'complete real session cookie set')
            self.cookies = cookies
        return status, value

    def result(self, method, path, status, body=None, action=None):
        actual, value = self.call(method, path, body, action)
        require(actual == status and value.get('ok') is True and type(value.get('result')) is dict,
                'Human journey', f'{method} {path} successful status {status} (received {actual})')
        return value['result']


class Authenticator:
    def __init__(self, source, origin):
        self.process = subprocess.Popen(['node', str(source), origin], stdin=subprocess.PIPE,
            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        self.selector = selectors.DefaultSelector()
        self.selector.register(self.process.stdout, selectors.EVENT_READ)

    def credential(self, operation, ceremony):
        wire = json.dumps(dict(operation=operation, ceremony=ceremony), separators=(',', ':')).encode() + b'\n'
        require(len(wire) <= 16384, 'Human journey', 'authenticator input bound')
        self.process.stdin.write(wire)
        self.process.stdin.flush()
        deadline, data = time.monotonic() + 10, bytearray()
        while b'\n' not in data:
            remaining = deadline - time.monotonic()
            require(remaining > 0 and self.selector.select(remaining), 'Human journey', 'authenticator deadline')
            chunk = os.read(self.process.stdout.fileno(), 16385 - len(data))
            require(chunk and len(data) + len(chunk) <= 16384, 'Human journey', 'authenticator output bound')
            data.extend(chunk)
        require(data.count(b'\n') == 1 and data.endswith(b'\n'), 'Human journey', 'one authenticator response')
        value = json.loads(data, object_pairs_hook=strict_pairs)
        fields(value, 'credential', 'Human journey', 'actual authenticator response')
        require(type(value['credential']) is str and re.fullmatch('[A-Za-z0-9_-]+', value['credential']),
                'Human journey', 'encoded credential')
        return value['credential']

    def close(self):
        self.process.stdin.close()
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=5)
        self.selector.close()
        self.process.stdout.close()


def journey(args):
    config = protected_json(args.request)
    fields(config, 'email display_name idempotency_key sponsor_principal initial_funding', args.request, 'fresh owner request')
    require(type(config['initial_funding']) is int and 0 < config['initial_funding'] < 2**128,
            args.request, 'bounded native funding')
    api = Api(args.url, args.ca, args.origin)
    if args.resume:
        retained = protected_json(args.result)
        token = protected_bytes(args.credential, 4096).decode()
        require(token and not any(ord(c) < 33 or ord(c) > 126 for c in token),
                'Human journey', 'retained real session credential')
        api.cookies = {'__Host-layerx_access': token}
        profile = api.result('GET', '/v1/profile', 200)
        state = api.result('GET', '/v1/onboarding', 200)
        balance = api.result('GET', '/v1/account/balance', 200)
        require(profile['display_name'] == config['display_name'] and state == retained['journey']
                and balance['account_id'] == retained['account_id']
                and balance['verification'] == 'checkpoint-finalised',
                'Human journey', 'retained authenticated principal and executed onboarding')
        return
    status, _ = api.call('GET', '/v1/account/balance', authenticated=False)
    require(status == 401, 'Human journey', 'unauthenticated balance refusal')
    body = {name: config[name] for name in ('email', 'display_name')}
    created = api.result('POST', '/v1/accounts', 201, body, config['idempotency_key'])
    principal = created['account_id']
    require(type(principal) is str and re.fullmatch('[a-z0-9_-]{1,128}', principal)
            and principal != config['sponsor_principal'], 'Human journey', 'new distinct principal')
    repeated = api.result('POST', '/v1/accounts', 201, body, config['idempotency_key'])
    require(repeated['account_id'] == principal, 'Human journey', 'account retry identity')
    authenticator = Authenticator(args.authenticator, args.origin)
    try:
        registration = api.result('POST', '/v1/passkeys/registrations', 200, dict(account_id=principal))
        credential = authenticator.credential('register', registration['ceremony'])
        passkey = api.result('POST', '/v1/passkeys/registrations/' + registration['registration_id'],
            200, dict(credential=credential))
        assertion = api.result('POST', '/v1/passkeys/assertions', 200, dict(email=config['email']))
        credential = authenticator.credential('assert', assertion['ceremony'])
        proof = api.result('POST', '/v1/passkeys/assertions/' + assertion['assertion_id'],
            200, dict(credential=credential))
        require(proof['assertion_id'] == assertion['assertion_id']
                and proof['passkey_id'] == passkey['passkey_id'], 'Human journey', 'actual passkey assertion binding')
        session_body = dict(assertion_id=proof['assertion_id'], device=dict(label='LayerX web app', platform='web'))
        action = 'session-open:' + proof['assertion_id']
        deadline = time.monotonic() + 120
        while True:
            status, value = api.call('POST', '/v1/sessions', session_body, action)
            if status == 201:
                require(value.get('ok') is True and api.cookies, 'Human journey', 'executed session with real cookies')
                session = value['result']
                break
            require(status == 503 and time.monotonic() < deadline, 'Human journey',
                    f'bounded durable onboarding progress (received {status})')
            time.sleep(.2)
        profile = api.result('GET', '/v1/profile', 200)
        require(profile['display_name'] == config['display_name'], 'Human journey', 'principal profile isolation')
        state = api.result('GET', '/v1/onboarding', 200)
        require(state['kind'] == 'onboarding' and state['state'] == 'complete' and state['account_active'] is True,
                'Human journey', 'executed native owner and recovery')
        stages = {stage['stage']: stage for stage in state['stages']}
        for name in ('protocol-identity', 'initial-funding', 'recovery'):
            require(stages[name]['state'] == 'receipt-verified' and any(item['class'] == 'layerx-receipt'
                and item['verification'] == 'receipt-verified' for item in stages[name]['evidence']),
                'Human journey', 'original native ' + name + ' evidence')
        balance = api.result('GET', '/v1/account/balance', 200)
        canonical = ('agent:did:layerx:' + principal + ':main').encode()
        account = hashlib.sha256(b'LX:ACCOUNT:v1' + len(canonical).to_bytes(4, 'big') + canonical).hexdigest()
        require(balance['account_id'] == 'act_' + account and balance['verification'] == 'checkpoint-finalised'
                and balance['freshness']['within_bound'] is True and balance['freshness']['checkpoint'] != '00' * 32
                and 0 < int(balance['money']['amount']) <= config['initial_funding'] and balance['evidence'],
                'Human journey', 'funded principal MAIN with canonical finality evidence')
        replay = api.result('POST', '/v1/sessions', 201, session_body, action)
        require(replay['session_id'] == session['session_id'], 'Human journey', 'session retry identity')
        after = api.result('GET', '/v1/account/balance', 200)
        require(after['account_id'] == balance['account_id'] and after['money'] == balance['money'],
                'Human journey', 'session replay does not debit or credit')
        sessions = api.result('GET', '/v1/sessions', 200)['sessions']
        require(len(sessions) == 1 and sessions[0]['session_id'] == session['session_id'],
                'Human journey', 'one executed session')
        credential_path = Path(args.credential)
        require(credential_path.is_absolute() and credential_path.resolve() == credential_path,
                credential_path, 'canonical private credential destination')
        from owner_native import protected_write
        protected_write(credential_path, api.cookies['__Host-layerx_access'].encode())
        write_json(Path(args.result), dict(principal=principal, account_id=balance['account_id'],
            journey=state, balance=balance, session_id=session['session_id']))
    finally:
        authenticator.close()


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    for name in ('url', 'ca', 'origin', 'request', 'authenticator', 'credential', 'result'):
        parser.add_argument('--' + name, required=True)
    parser.add_argument('--resume', action='store_true')
    try:
        journey(parser.parse_args())
    except Exception:
        sys.stderr.write('Actual Human onboarding journey refused\n')
        raise SystemExit(1) from None
