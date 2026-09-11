#!/usr/bin/env python3
# Signing providers for the LayerX treasury signer.
#
# A provider owns one Ed25519 identity and answers two questions: the public
# key it signs under, and the 64-byte signature over a 32-byte digest. The
# file provider keeps the seed in anonymous memory for the life of the
# process and never writes it back to disk. The command provider delegates
# both answers to an external program, which is how a KMS or an HSM is placed
# behind the signer without changing the socket contract:
#
#   PROGRAM public-key        -> 64 lowercase hex characters on standard output
#   PROGRAM sign <64 hex>     -> 128 lowercase hex characters on standard output
#
# The signer verifies every signature a provider returns against the public
# key the provider advertised before it answers a client.
import os
import shlex
import stat
import subprocess

PKCS8_ED25519_PREFIX = bytes.fromhex('302e020100300506032b657004220420')
SPKI_ED25519_PREFIX = bytes.fromhex('302a300506032b6570032100')
MAXIMUM_MATERIAL_BYTES = 4096
MAXIMUM_COMMAND_OUTPUT_BYTES = 4096


class ProviderError(Exception):
    pass


def openssl(arguments, descriptors=(), stdin=None, maximum=MAXIMUM_COMMAND_OUTPUT_BYTES):
    result = subprocess.run(['openssl'] + arguments, input=stdin, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, pass_fds=tuple(descriptors), check=False)
    if result.returncode != 0 or len(result.stdout) > maximum:
        raise ProviderError('openssl refused a treasury key operation')
    return result.stdout


def memory_file(name, value):
    descriptor = os.memfd_create(name, 0)
    try:
        os.write(descriptor, value)
        os.lseek(descriptor, 0, os.SEEK_SET)
    except BaseException:
        os.close(descriptor)
        raise
    return descriptor


def parse_hex(text, length):
    value = text.strip()
    if len(value) != length * 2 or value != value.lower():
        raise ProviderError('provider output is not lowercase hexadecimal of the required length')
    try:
        decoded = bytes.fromhex(value)
    except ValueError as error:
        raise ProviderError('provider output is not hexadecimal') from error
    return decoded


def verify(public_key, digest, signature):
    if len(public_key) != 32 or len(digest) != 32 or len(signature) != 64:
        return False
    key = memory_file('signer-public', SPKI_ED25519_PREFIX + public_key)
    try:
        message = memory_file('signer-digest', digest)
        try:
            proof = memory_file('signer-signature', signature)
            try:
                result = subprocess.run(
                    ['openssl', 'pkeyutl', '-verify', '-rawin', '-pubin', '-keyform', 'DER',
                     '-inkey', f'/proc/self/fd/{key}', '-in', f'/proc/self/fd/{message}',
                     '-sigfile', f'/proc/self/fd/{proof}'],
                    stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                    pass_fds=(key, message, proof), check=False)
            finally:
                os.close(proof)
        finally:
            os.close(message)
    finally:
        os.close(key)
    return result.returncode == 0


class Provider:
    name = 'provider'

    def public_key(self):
        raise NotImplementedError

    def sign(self, digest):
        raise NotImplementedError

    def close(self):
        return None


class FileProvider(Provider):
    name = 'file'

    def __init__(self, path):
        self._key = None
        seed = self._load(path)
        try:
            self._key = memory_file('treasury-signer-key', PKCS8_ED25519_PREFIX + bytes(seed))
        finally:
            for index in range(len(seed)):
                seed[index] = 0

    @staticmethod
    def _load(path):
        descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
        try:
            info = os.fstat(descriptor)
            mode = stat.S_IMODE(info.st_mode)
            if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
                raise ProviderError('treasury key material must be a regular file with one link')
            if mode & 0o007 or mode & 0o020:
                raise ProviderError('treasury key material must not be readable by others '
                                    'or writable by its group')
            if info.st_uid not in (os.geteuid(), 0):
                raise ProviderError('treasury key material must be owned by the signer or by root')
            if info.st_size > MAXIMUM_MATERIAL_BYTES:
                raise ProviderError('treasury key material exceeds its bound')
            raw = bytearray(os.read(descriptor, MAXIMUM_MATERIAL_BYTES + 1))
        finally:
            os.close(descriptor)
        if len(raw) == 32:
            return raw
        text = bytes(raw).decode('ascii', 'replace').strip()
        for index in range(len(raw)):
            raw[index] = 0
        if len(text) != 64 or text != text.lower():
            raise ProviderError('treasury key material must hold 32 raw bytes or 64 hex characters')
        try:
            return bytearray(bytes.fromhex(text))
        except ValueError as error:
            raise ProviderError('treasury key material is not hexadecimal') from error

    def public_key(self):
        encoded = openssl(['pkey', '-inform', 'DER', '-in', f'/proc/self/fd/{self._key}',
                           '-pubout', '-outform', 'DER'], (self._key,))
        if len(encoded) != 44 or not encoded.startswith(SPKI_ED25519_PREFIX):
            raise ProviderError('treasury key material is not an Ed25519 identity')
        return encoded[12:]

    def sign(self, digest):
        message = memory_file('treasury-signer-digest', digest)
        try:
            signature = openssl(['pkeyutl', '-sign', '-rawin', '-keyform', 'DER',
                                 '-inkey', f'/proc/self/fd/{self._key}',
                                 '-in', f'/proc/self/fd/{message}'], (self._key, message))
        finally:
            os.close(message)
        if len(signature) != 64:
            raise ProviderError('the treasury signature has an unexpected length')
        return signature

    def close(self):
        if self._key is not None:
            os.close(self._key)
            self._key = None


class CommandProvider(Provider):
    name = 'command'

    def __init__(self, command, timeout):
        self._command = shlex.split(command)
        if not self._command:
            raise ProviderError('the provider command is empty')
        if timeout <= 0:
            raise ProviderError('the provider timeout must be positive')
        self._timeout = timeout

    def _run(self, arguments):
        try:
            result = subprocess.run(self._command + arguments, stdout=subprocess.PIPE,
                                    stderr=subprocess.PIPE, timeout=self._timeout, check=False)
        except (OSError, subprocess.SubprocessError) as error:
            raise ProviderError('the provider command did not answer') from error
        if result.returncode != 0 or len(result.stdout) > MAXIMUM_COMMAND_OUTPUT_BYTES:
            raise ProviderError('the provider command refused the request')
        return result.stdout.decode('ascii', 'replace')

    def public_key(self):
        return parse_hex(self._run(['public-key']), 32)

    def sign(self, digest):
        return parse_hex(self._run(['sign', digest.hex()]), 64)


def load(name, key_file, command, timeout):
    if name == 'file':
        if not key_file or command:
            raise ProviderError('the file provider takes a key file and no provider command')
        return FileProvider(key_file)
    if name == 'command':
        if not command or key_file:
            raise ProviderError('the command provider takes a provider command and no key file')
        return CommandProvider(command, timeout)
    raise ProviderError('the provider must be file or command')
