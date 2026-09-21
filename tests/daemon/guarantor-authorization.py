#!/usr/bin/env python3
import copy
import importlib.util
import json
import os
from pathlib import Path
import socket
import stat
import subprocess
import sys
import tempfile
import textwrap
import threading
import time
import unittest

from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / 'tests/fixtures/custody/paxeer-light-v1'
SIGN = ROOT / 'cmd/layerx-guarantor/publication-sign.py'
GUARANTOR = ROOT / 'platform/hosted/node/guarantor.sh'
TREASURY_SIGNER = ROOT / 'platform/hosted/node/signer/signer.py'
sys.path[:0] = [str(ROOT / 'platform/hosted/node'), str(ROOT / 'platform/hosted/human')]


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


s = load('authorization_settlement', ROOT / 'cmd/layerx-guarantor/settlement.py')
p = load('authorization_codec', ROOT / 'cmd/layerx-guarantor/publication.py')
a = load('authorization_policy', ROOT / 'cmd/layerx-guarantor/authorization.py')
t = load('authorization_publication_tests', ROOT / 'tests/daemon/guarantor-publication.py')
import onboarding_socket


def public(key):
    return key.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)


def others():
    return [p.state_leaf(index.to_bytes(2, 'big'), t.sha(b'LXP/v1/state-subtree\0' + index.to_bytes(2, 'big')))
            for index in range(9)]


def owner_record(principal, authority, asset):
    name = ('agent:did:layerx:' + principal + ':main').encode()
    account = p.account_id(name)
    tail = (b'\x01' + (1234).to_bytes(16, 'big') + asset + b'\x01' + bytes(16) + b'\x00\x00' + authority + b'\x01')
    return account, b'\x04' + account, len(name).to_bytes(2, 'big') + name + tail


def account_witnesses(records, modules):
    """Module-0 account witnesses for several accounts under one account tree and one state root."""
    leaves = [p.state_leaf(key, value) for key, value in records]
    _, account_root = t.merkle(leaves, 0)
    leaf_path, subtree = t.merkle([p.state_leaf(b'account-tree', account_root)], 0)
    modules = list(modules)
    modules[0] = p.state_leaf((0).to_bytes(2, 'big'), subtree)
    module_path, root = t.merkle(modules, 0)
    encoded = []
    for index, (key, value) in enumerate(records):
        account_path, _ = t.merkle(leaves, index)
        encoded.append(p.hx((2).to_bytes(2, 'big') + (0).to_bytes(2, 'big') +
                            len(key).to_bytes(4, 'big') + key + len(value).to_bytes(4, 'big') + value +
                            index.to_bytes(4, 'big') + len(leaves).to_bytes(4, 'big') +
                            len(account_path).to_bytes(1, 'big') + b''.join(account_path) +
                            (0).to_bytes(4, 'big') + (1).to_bytes(4, 'big') +
                            len(leaf_path).to_bytes(1, 'big') + b''.join(leaf_path) +
                            len(modules).to_bytes(4, 'big') +
                            len(module_path).to_bytes(1, 'big') + b''.join(module_path)))
    return encoded, root


def seed_of(key):
    return key.private_bytes(serialization.Encoding.Raw, serialization.PrivateFormat.Raw, serialization.NoEncryption())


def secret(path, data):
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, 'wb') as out:
        out.write(data)
    return path


class PublicationInputsDirectoryTests(unittest.TestCase):
    def test_a_setgid_state_directory_still_yields_a_private_inputs_directory(self):
        digest = t.sha(b'inputs directory checkpoint')
        with tempfile.TemporaryDirectory() as temporary:
            state = Path(temporary).resolve() / 'state'
            state.mkdir()
            state.chmod(0o2770)
            self.assertEqual(stat.S_IMODE(state.lstat().st_mode), 0o2770)
            inherited = state / 'inherited'
            inherited.mkdir()
            subprocess.run(['chmod', '0700', str(inherited)], check=True)
            self.assertEqual(stat.S_IMODE(inherited.lstat().st_mode), 0o2700)
            with self.assertRaisesRegex(ValueError, 'authorization output directory is not private'):
                a.private_output(inherited / (digest.hex() + '.json'), digest)
            inputs = state / 'producer' / 'publication-inputs'
            for _ in range(2):
                subprocess.run(['bash', str(GUARANTOR), '--publication-inputs-dir', str(inputs)], check=True)
                self.assertEqual(stat.S_IMODE(inputs.lstat().st_mode), 0o700)
            a.private_output(inputs / (digest.hex() + '.json'), digest)
            with self.assertRaisesRegex(ValueError, 'authorization output checkpoint mismatch'):
                a.private_output(inputs / 'other.json', digest)

    def test_the_producer_loop_prepares_its_inputs_directory_the_same_way(self):
        source = GUARANTOR.read_text()
        self.assertIn('private_directory "$LAYERX_GUARANTOR_PUBLICATION_INPUTS_DIR"', source)
        self.assertNotIn('chmod 0700 "$LAYERX_GUARANTOR_PUBLICATION_INPUTS_DIR"', source)


class RecipientSignerTests(unittest.TestCase):
    NETWORK = 7
    PRINCIPAL = 'self-custodied-owner'

    def setUp(self):
        self.key = Ed25519PrivateKey.generate()
        self.asset = t.sha(b'recipient signer asset')
        self.digest = t.sha(b'recipient signer checkpoint')
        self.account, key, value = owner_record(self.PRINCIPAL, public(self.key), self.asset)
        encoded, root = t.balance_witness(key, value, others())
        self.header = s.values(s.HEADER_TYPES, [2, self.NETWORK, 7, 11, 1, 1000] + [p.hx(bytes(32)), p.hx(root)]
                               + [p.hx(bytes(32))] * 5 + [1, p.hx(bytes(32))])
        self.assertEqual(self.header[7], root)
        self.fact = p.balance_fact(encoded, root)
        self.assertEqual((self.fact['account'], self.fact['authority']), (self.account, public(self.key)))
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.work = Path(self.directory.name).resolve()
        self.socket = self.work / 'recipient.sock'

    def policy(self, human=True, treasury_public='ab' * 32):
        return dict(treasury=dict(socket=str(self.work / 'treasury.sock'), peer_uid=os.geteuid(),
                                  peer_gid=os.getegid(), public_key=treasury_public,
                                  asset_id=self.asset.hex(), recipient='56' * 20),
                    human=dict(socket=str(self.socket), peer_uid=os.geteuid(), peer_gid=os.getegid())
                    if human else None)

    def bind(self, policy):
        return a.recipient_binding(s, p, policy, self.fact, self.header, self.digest)

    def serve(self, principal, executable):
        listener = socket.socket(socket.AF_UNIX, socket.SOCK_SEQPACKET)
        listener.bind(str(self.socket))
        self.socket.chmod(0o660)
        listener.listen(1)
        configuration = dict(client_uid=os.geteuid(), client_gid=os.getegid(), principal=principal)

        def answer():
            connection, _ = listener.accept()
            with connection:
                onboarding_socket.serve_request(connection, configuration, executable, 'settlement-recipient')

        thread = threading.Thread(target=answer, daemon=True)
        thread.start()
        self.addCleanup(listener.close)
        self.addCleanup(thread.join, 30)

    def signer(self, signing_key):
        seed = signing_key.private_bytes(serialization.Encoding.Raw, serialization.PrivateFormat.Raw,
                                         serialization.NoEncryption()).hex()
        script = self.work / 'recipient-signer.py'
        script.write_text(textwrap.dedent('''\
            #!%s
            import hashlib, json, sys
            from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
            body = json.load(sys.stdin)
            name = ('agent:did:layerx:' + body['principal'] + ':main').encode()
            account = hashlib.sha256(b'LX:ACCOUNT:v1' + len(name).to_bytes(4, 'big') + name).digest()
            asset, checkpoint, recipient = bytes(body['asset']), bytes(body['checkpoint']), bytes([0x56]) * 20
            message = b'LX:SETTLE:RECIPIENT:v1\\0' + (%d).to_bytes(4, 'big') + account + asset + recipient + checkpoint
            signed = Ed25519PrivateKey.from_private_bytes(bytes.fromhex('%s')).sign(message)
            json.dump(dict(network_id=%d, principal=body['principal'], did='did:layerx:' + body['principal'],
                           account=account.hex(), public_key='%s', asset=asset.hex(), checkpoint=checkpoint.hex(),
                           recipient=recipient.hex(), signature=signed.hex()), sys.stdout)
            ''') % (sys.executable, self.NETWORK, seed, self.NETWORK, public(self.key).hex()))
        script.chmod(0o700)
        return script

    def test_absent_recipient_signer_is_pending(self):
        with self.assertRaises(s.AuthorizationPending) as caught:
            self.bind(self.policy())
        self.assertNotIsInstance(caught.exception, ValueError)
        self.assertIn(self.PRINCIPAL, str(caught.exception))
        self.assertIn(str(self.socket), str(caught.exception))

    def test_recipient_signer_that_is_not_listening_is_pending(self):
        listener = socket.socket(socket.AF_UNIX, socket.SOCK_SEQPACKET)
        listener.bind(str(self.socket))
        self.socket.chmod(0o660)
        listener.close()
        with self.assertRaises(s.AuthorizationPending):
            self.bind(self.policy())

    def test_policy_without_a_recipient_signer_awaits_the_owner_file(self):
        with self.assertRaises(s.AuthorizationPending) as caught:
            self.bind(self.policy(human=False))
        self.assertIn(self.digest.hex(), str(caught.exception))

    def test_absent_treasury_signer_is_pending(self):
        treasury = Ed25519PrivateKey.generate()
        principal = public(treasury).hex()
        account, key, value = owner_record(principal, public(treasury), self.asset)
        encoded, root = t.balance_witness(key, value, others())
        header = list(self.header)
        header[7] = root
        fact = p.balance_fact(encoded, root)
        with self.assertRaises(s.AuthorizationPending) as caught:
            a.recipient_binding(s, p, self.policy(treasury_public=principal), fact, tuple(header), self.digest)
        self.assertIn('treasury signer', str(caught.exception))

    def test_pending_leaves_the_process_with_the_wait_status(self):
        self.assertEqual(s.AUTHORIZATION_PENDING_EXIT, 75)
        source = (ROOT / 'cmd/layerx-guarantor/settlement.py').read_text()
        self.assertRegex(source, r'except AuthorizationPending as error:\n(.*\n)?\s*sys\.exit\(AUTHORIZATION_PENDING_EXIT\)')

    def test_refusing_recipient_signer_is_a_refusal(self):
        self.serve('another-principal', self.signer(self.key))
        with self.assertRaises(ValueError) as caught:
            self.bind(self.policy())
        self.assertNotIsInstance(caught.exception, s.AuthorizationPending)
        self.assertIn('successful signer response', str(caught.exception))

    def test_recipient_signer_with_the_wrong_key_is_a_refusal(self):
        self.serve(self.PRINCIPAL, self.signer(Ed25519PrivateKey.generate()))
        with self.assertRaises(InvalidSignature):
            self.bind(self.policy())

    def test_answering_recipient_signer_yields_a_binding_the_producer_verifies(self):
        self.serve(self.PRINCIPAL, self.signer(self.key))
        binding = self.bind(self.policy())
        authorization = dict(version=2, checkpoint_id=p.hx(self.digest), recipient_bindings=[binding],
                             deposit_registration=None)
        verified = p.verified_bindings(authorization, [self.fact], self.header, self.digest)
        self.assertEqual(verified[0][1:3], (bytes([0x56]) * 20, self.digest))


class ComposedAuthorizationTests(unittest.TestCase):
    """One checkpoint holding a treasury balance, signed over the real treasury signer socket, and a
    self-custodied owner balance, delivered as a signed partial file, under a policy with no
    recipient signer."""
    NETWORK = 7
    OWNER_RECIPIENT = '0x' + '9a' * 20

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.work = Path(self.directory.name).resolve()
        self.treasury, self.owner = Ed25519PrivateKey.generate(), Ed25519PrivateKey.generate()
        self.asset = t.sha(b'composed authorization asset')
        records = []
        for principal, key in ((public(self.treasury).hex(), self.treasury), ('self-custodied-owner', self.owner)):
            _, leaf_key, value = owner_record(principal, public(key), self.asset)
            records.append((leaf_key, value))
        encoded, root = account_witnesses(records, others())
        header = ([2, self.NETWORK, 7, 11, 1, 1000] + [p.hx(bytes(32)), p.hx(root)] + [p.hx(bytes(32))] * 5
                  + [1, p.hx(bytes(32))])
        self.header = s.values(s.HEADER_TYPES, header)
        proof = b'composed authorization validity proof'
        self.digest = s.checkpoint_hash(self.header, proof)
        self.request = dict(chain_id=125, header=header, checkpoint_id=p.hx(self.digest), validity_proof=p.hx(proof),
                            attestations=[], native_facts={'balances': encoded, 'withdrawals': [], 'deposits': [],
                                                           'profile': None})
        self.balances, _, _, _ = p.native_request(s, self.request, self.header, self.digest)
        self.assertEqual({fact['authority'] for fact in self.balances}, {public(self.treasury), public(self.owner)})
        self.inputs = self.work / 'inputs'
        self.inputs.mkdir(mode=0o700)
        self.source = self.inputs / (self.digest.hex() + '.json')
        self.manifest = self.work / (self.digest.hex() + '.publication-request.json')
        self.manifest.write_text(json.dumps(self.request))
        self.owner_file = secret(self.work / 'owner.key', seed_of(self.owner).hex().encode())
        self.socket = self.work / 'treasury-signer.sock'
        self.policy = dict(treasury=dict(socket=str(self.socket), peer_uid=os.geteuid(), peer_gid=os.getegid(),
                                         public_key=public(self.treasury).hex(), asset_id=self.asset.hex(),
                                         recipient='56' * 20), human=None)

    def start_treasury_signer(self):
        key = secret(self.work / 'treasury.key', seed_of(self.treasury))
        binding = secret(self.work / 'binding.json', json.dumps(dict(
            version=1, network_id=self.NETWORK, asset_id=self.asset.hex(), recipient='56' * 20)).encode())
        log = (self.work / 'treasury-signer.log').open('wb')
        self.addCleanup(log.close)
        process = subprocess.Popen([sys.executable, str(TREASURY_SIGNER), '--socket', str(self.socket),
                                    '--allowed-uid', str(os.getuid()), '--provider', 'file', '--key-file', str(key),
                                    '--binding-policy', str(binding)], stdout=subprocess.PIPE, stderr=log, cwd=str(ROOT))
        self.addCleanup(process.wait, 30)
        self.addCleanup(process.terminate)
        deadline = time.monotonic() + 30
        while not self.socket.is_socket():
            self.assertIsNone(process.poll(), (self.work / 'treasury-signer.log').read_text())
            self.assertLess(time.monotonic(), deadline)
            time.sleep(.05)

    def deliver(self):
        completed = subprocess.run([sys.executable, str(SIGN), str(self.manifest), str(self.inputs), '--owner',
                                    str(self.owner_file) + '=' + self.OWNER_RECIPIENT, '--partial'],
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        self.assertEqual(completed.returncode, 0, completed.stderr)
        return self.inputs / (self.digest.hex() + '.partial.json')

    def compose(self):
        delivered, deposit = a.delivered_partial(p, self.source, self.digest)
        self.assertIsNone(deposit)
        return a.recipient_bindings(s, p, self.policy, self.balances, self.header, self.digest, delivered)

    def test_socket_signed_and_delivered_bindings_compose_into_one_authorization(self):
        self.start_treasury_signer()
        with self.assertRaises(s.AuthorizationPending) as caught:
            self.compose()
        self.assertIn('self-custodied-owner', str(caught.exception))
        self.assertEqual(list(self.inputs.iterdir()), [])
        self.deliver()
        bindings = self.compose()
        authorization = dict(version=2, checkpoint_id=p.hx(self.digest), recipient_bindings=bindings,
                             deposit_registration=None)
        verified = p.verified_bindings(authorization, self.balances, self.header, self.digest)
        recipients = {fact['authority']: recipient for fact, recipient, _, _ in verified}
        self.assertEqual(recipients, {public(self.treasury): bytes([0x56]) * 20,
                                      public(self.owner): bytes.fromhex(self.OWNER_RECIPIENT[2:])})
        p.verified_deposit(authorization, [], None, self.header, self.digest)

    def test_delivered_binding_alone_stays_pending_while_the_treasury_signer_is_away(self):
        self.deliver()
        with self.assertRaises(s.AuthorizationPending) as caught:
            self.compose()
        self.assertIn('treasury signer', str(caught.exception))
        self.assertFalse(self.source.exists())

    def test_tampered_delivered_binding_is_a_refusal_even_with_the_treasury_signer_up(self):
        self.start_treasury_signer()
        partial = self.deliver()
        value = json.loads(partial.read_text())
        value['recipient_bindings'][0]['recipient'] = '0x' + '9b' * 20
        partial.write_text(json.dumps(value))
        with self.assertRaisesRegex(ValueError, 'publication signature invalid'):
            self.compose()
        value['checkpoint_id'] = p.hx(t.sha(b'another checkpoint'))
        partial.write_text(json.dumps(value))
        with self.assertRaisesRegex(ValueError, 'delivered authorization version or checkpoint'):
            self.compose()

    def test_the_owner_tool_alone_never_writes_the_final_file_for_a_mixed_checkpoint(self):
        completed = subprocess.run([sys.executable, str(SIGN), str(self.manifest), str(self.inputs), '--owner',
                                    str(self.owner_file) + '=' + self.OWNER_RECIPIENT],
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        self.assertNotEqual(completed.returncode, 0)
        self.assertIn('incomplete', completed.stderr)
        self.assertEqual(list(self.inputs.iterdir()), [])


class PublicationSigningToolTests(unittest.TestCase):
    RECIPIENT = '0x' + '9a' * 20

    def setUp(self):
        profile = (FIXTURES / 'custody.profile').read_bytes()
        credit = (FIXTURES / 'custody.credit').read_bytes()[:p.CREDIT_BYTES]
        nullifier = bytes.fromhex((FIXTURES / 'custody.credit.nullifier').read_text().strip())
        network, protocol = int.from_bytes(profile[201:205], 'big'), int.from_bytes(profile[205:207], 'big')
        self.profile = profile
        self.owner = Ed25519PrivateKey.generate()
        self.authority = Ed25519PrivateKey.generate()
        asset = t.sha(b'signing tool asset')
        account, key, value = owner_record('signing-tool-owner', public(self.owner), asset)
        _, account_root = t.merkle([p.state_leaf(key, value)], 0)
        _, account_subtree = t.merkle([p.state_leaf(b'account-tree', account_root)], 0)
        credit_key = b'deposit-nullifier:' + nullifier
        custody = [p.state_leaf(p.PROFILE_KEY, profile), p.state_leaf(credit_key, credit)]
        _, custody_subtree = t.merkle(custody, 0)
        modules = others()
        modules[0] = p.state_leaf((0).to_bytes(2, 'big'), account_subtree)
        modules[8] = p.state_leaf((8).to_bytes(2, 'big'), custody_subtree)
        balance, root = t.balance_witness(key, value, modules)
        profile_witness, profile_root = t.encode(8, p.PROFILE_KEY, profile, custody, 0, modules, 8)
        credit_witness, credit_root = t.encode(8, credit_key, credit, custody, 1, modules, 8)
        self.assertEqual((profile_root, credit_root), (root, root))
        header = [protocol, network, 7, 11, 1, 1000] + [p.hx(bytes(32)), p.hx(root)] + [p.hx(bytes(32))] * 5 + [1, p.hx(bytes(32))]
        self.header = s.values(s.HEADER_TYPES, header)
        proof = b'signing tool validity proof'
        self.digest = s.checkpoint_hash(self.header, proof)
        self.request = dict(chain_id=125, header=header, checkpoint_id=p.hx(self.digest), validity_proof=p.hx(proof),
                            attestations=[], native_facts={'balances': [balance], 'withdrawals': [],
                                                           'deposits': [credit_witness], 'profile': profile_witness})
        self.balances, _, self.deposits, _ = p.native_request(s, self.request, self.header, self.digest)
        self.assertEqual((len(self.balances), len(self.deposits)), (1, 1))
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.work = Path(self.directory.name).resolve()
        self.inputs = self.work / 'inputs'
        self.inputs.mkdir(mode=0o700)
        self.manifest = self.work / (self.digest.hex() + '.publication-request.json')
        self.manifest.write_text(json.dumps(self.request))
        self.owner_file = self.secret('owner.key', self.owner.private_bytes(
            serialization.Encoding.Raw, serialization.PrivateFormat.Raw, serialization.NoEncryption()).hex().encode())
        self.authority_file = self.secret('checkpoint-authority.pem', self.authority.private_bytes(
            serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8, serialization.NoEncryption()))

    def secret(self, name, data):
        path = self.work / name
        descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, 'wb') as out:
            out.write(data)
        return path

    def sign(self, *options, output=None):
        return subprocess.run([sys.executable, str(SIGN), str(self.manifest), str(output or self.inputs), *options],
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)

    def verify(self, authorization):
        verified = p.verified_bindings(authorization, self.balances, self.header, self.digest)
        message, signed, ordering, vault, _ = p.verified_deposit(authorization, self.deposits, self.profile,
                                                                 self.header, self.digest)
        p.signature(public(self.authority), message, signed)
        return verified, message, ordering, vault

    def signed(self):
        completed = self.sign('--owner', str(self.owner_file) + '=' + self.RECIPIENT,
                              '--checkpoint-authority-key', str(self.authority_file))
        self.assertEqual(completed.returncode, 0, completed.stderr)
        path = self.inputs / (self.digest.hex() + '.json')
        self.assertEqual(completed.stdout.strip(), str(path))
        self.assertEqual(stat.S_IMODE(path.lstat().st_mode), 0o600)
        return p.read_authorizations(path)

    def test_tool_signed_authorization_passes_the_producer_verification(self):
        authorization = self.signed()
        self.assertEqual(set(authorization), {'version', 'checkpoint_id', 'recipient_bindings', 'deposit_registration'})
        verified, message, ordering, vault = self.verify(authorization)
        self.assertEqual(verified[0][1:3], (bytes.fromhex(self.RECIPIENT[2:]), self.digest))
        self.assertEqual(p.raw(vault, 20), self.profile[13:33])
        self.assertEqual(len(ordering), 1)
        reference = bytes(12) + self.profile[13:33]
        self.assertEqual(message, a.deposit_message(p, self.deposits, self.header, self.digest, reference))

    def test_tampered_authorization_is_refused(self):
        authorization = self.signed()
        for name, value in (('recipient', '0x' + '9b' * 20), ('request_anchor', p.hx(t.sha(b'other anchor'))),
                            ('signature', p.hx(bytes(64)))):
            with self.subTest(binding=name):
                changed = copy.deepcopy(authorization)
                changed['recipient_bindings'][0][name] = value
                with self.assertRaisesRegex(ValueError, 'publication signature invalid'):
                    self.verify(changed)
        changed = copy.deepcopy(authorization)
        changed['deposit_registration']['custody_reference'] = p.hx(t.sha(b'other custody reference'))
        with self.assertRaisesRegex(ValueError, 'publication signature invalid'):
            self.verify(changed)
        changed = copy.deepcopy(authorization)
        changed['deposit_registration']['vault'] = '0x' + '77' * 20
        with self.assertRaisesRegex(ValueError, 'deposit vault mismatch'):
            self.verify(changed)
        changed = copy.deepcopy(authorization)
        changed['checkpoint_id'] = p.hx(t.sha(b'other checkpoint'))
        with self.assertRaisesRegex(ValueError, 'publication authorization version or checkpoint'):
            self.verify(changed)
        changed = copy.deepcopy(authorization)
        changed['recipient_bindings'] = []
        with self.assertRaisesRegex(ValueError, 'complete recipient bindings required'):
            self.verify(changed)
        changed = copy.deepcopy(authorization)
        changed['deposit_registration'] = None
        with self.assertRaises(TypeError):
            self.verify(changed)

    def test_signature_by_another_owner_key_is_refused_by_the_tool(self):
        stranger = self.secret('stranger.key', Ed25519PrivateKey.generate().private_bytes(
            serialization.Encoding.Raw, serialization.PrivateFormat.Raw, serialization.NoEncryption()).hex().encode())
        completed = self.sign('--owner', str(stranger) + '=' + self.RECIPIENT,
                              '--checkpoint-authority-key', str(self.authority_file))
        self.assertNotEqual(completed.returncode, 0)
        self.assertIn('holds no balance', completed.stderr)
        self.assertEqual(list(self.inputs.iterdir()), [])

    def test_incomplete_authorization_is_never_written_as_the_final_file(self):
        completed = self.sign('--owner', str(self.owner_file) + '=' + self.RECIPIENT)
        self.assertNotEqual(completed.returncode, 0)
        self.assertIn('deposit registration', completed.stderr)
        self.assertEqual(list(self.inputs.iterdir()), [])
        handoff = self.work / 'handoff'
        handoff.mkdir()
        completed = self.sign('--owner', str(self.owner_file) + '=' + self.RECIPIENT, '--partial', output=handoff)
        self.assertEqual(completed.returncode, 0, completed.stderr)
        partial = handoff / (self.digest.hex() + '.partial.json')
        self.assertEqual([path.name for path in handoff.iterdir()], [partial.name])
        completed = self.sign('--merge', str(partial), '--checkpoint-authority-key', str(self.authority_file))
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.verify(p.read_authorizations(self.inputs / (self.digest.hex() + '.json')))

    def test_merged_binding_that_does_not_verify_is_refused(self):
        handoff = self.work / 'handoff'
        handoff.mkdir()
        self.assertEqual(self.sign('--owner', str(self.owner_file) + '=' + self.RECIPIENT, '--partial',
                                   output=handoff).returncode, 0)
        partial = handoff / (self.digest.hex() + '.partial.json')
        value = json.loads(partial.read_text())
        value['recipient_bindings'][0]['recipient'] = '0x' + '9b' * 20
        partial.write_text(json.dumps(value))
        completed = self.sign('--merge', str(partial), '--checkpoint-authority-key', str(self.authority_file))
        self.assertNotEqual(completed.returncode, 0)
        self.assertIn('publication signature invalid', completed.stderr)
        self.assertEqual(list(self.inputs.iterdir()), [])

    def test_repeated_delivery_leaves_a_valid_authorization_alone(self):
        self.signed()
        path = self.inputs / (self.digest.hex() + '.json')
        before = path.read_bytes()
        again = self.sign('--owner', str(self.owner_file) + '=' + self.RECIPIENT,
                          '--checkpoint-authority-key', str(self.authority_file))
        self.assertEqual(again.returncode, 0, again.stderr)
        self.assertIn('already delivered', again.stdout)
        self.assertEqual(path.read_bytes(), before)
        loop = self.sign('--owner', str(self.owner_file) + '=' + self.RECIPIENT, '--partial')
        self.assertEqual(loop.returncode, 0, loop.stderr)
        self.assertEqual([item.name for item in self.inputs.iterdir()], [path.name])

    def test_repeated_partial_delivery_is_unchanged_and_a_different_file_is_not_overwritten(self):
        options = ('--owner', str(self.owner_file) + '=' + self.RECIPIENT, '--partial')
        self.assertEqual(self.sign(*options).returncode, 0)
        partial = self.inputs / (self.digest.hex() + '.partial.json')
        before = partial.read_bytes()
        again = self.sign(*options)
        self.assertEqual(again.returncode, 0, again.stderr)
        self.assertIn('already delivered', again.stdout)
        self.assertEqual(partial.read_bytes(), before)
        other = self.sign('--owner', str(self.owner_file) + '=0x' + '9b' * 20, '--partial')
        self.assertNotEqual(other.returncode, 0)
        self.assertIn('not overwritten', other.stderr)
        self.assertEqual(partial.read_bytes(), before)
        final = self.inputs / (self.digest.hex() + '.json')
        value = json.loads(before)
        value['recipient_bindings'][0]['signature'] = p.hx(bytes(64))
        final.write_text(json.dumps(value))
        invalid = final.read_bytes()
        refused = self.sign('--merge', str(partial), '--checkpoint-authority-key', str(self.authority_file))
        self.assertNotEqual(refused.returncode, 0)
        self.assertIn('not overwritten', refused.stderr)
        self.assertEqual(final.read_bytes(), invalid)

    def test_unprotected_key_and_altered_request_are_refused(self):
        self.owner_file.chmod(0o644)
        completed = self.sign('--owner', str(self.owner_file) + '=' + self.RECIPIENT,
                              '--checkpoint-authority-key', str(self.authority_file))
        self.assertNotEqual(completed.returncode, 0)
        self.assertIn('private 0600 file', completed.stderr)
        self.owner_file.chmod(0o600)
        request = copy.deepcopy(self.request)
        request['header'][2] += 1
        self.manifest.write_text(json.dumps(request))
        completed = self.sign('--owner', str(self.owner_file) + '=' + self.RECIPIENT,
                              '--checkpoint-authority-key', str(self.authority_file))
        self.assertNotEqual(completed.returncode, 0)
        self.assertIn('checkpoint hash mismatch', completed.stderr)
        self.assertEqual(list(self.inputs.iterdir()), [])


if __name__ == '__main__':
    unittest.main(verbosity=2)
