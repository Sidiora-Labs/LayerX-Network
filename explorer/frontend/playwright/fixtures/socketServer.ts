import type { TestFixture, Page } from '@playwright/test';
import type { WebSocket } from 'ws';
import { WebSocketServer } from 'ws';

import type { AddressCoinBalanceHistoryItem, AddressTokensBalancesSocketMessage } from 'types/api/address';
import type { NewBlockSocketResponse } from 'types/api/block';
import type { SmartContractVerificationResponse } from 'types/api/contract';
import type { TokenInstanceMetadataSocketMessage } from 'types/api/token';
import type { TokenTransfer } from 'types/api/tokenTransfer';
import type { Transaction } from 'types/api/transaction';

import { port as socketPort } from '../utils/socket';

export type CreateSocketFixture = () => Promise<WebSocket>;

type Channel = [string, string, string];

export interface SocketServerFixture {
  createSocket: CreateSocketFixture;
}

// eslint-disable-next-line @typescript-eslint/no-unused-vars
export const createSocket: TestFixture<CreateSocketFixture, { page: Page }> = async({ page }, use) => {
  const socketServer = new WebSocketServer({ port: socketPort });

  const connectionPromise = new Promise<WebSocket>((resolve) => {
    socketServer.on('connection', (socket: WebSocket) => {
      resolve(socket);
    });
  });

  await use(() => connectionPromise);

  socketServer.close();
};

export const joinChannel = async(socket: WebSocket, channelName: string) => {
  return new Promise<[string, string, string]>((resolve, reject) => {
    socket.on('message', (msg) => {
      try {
        const payload = JSON.parse(msg.toString()) as Array<string>;

        if (channelName === payload[2] && payload[3] === 'phx_join') {
          socket.send(JSON.stringify([
            payload[0],
            payload[1],
            payload[2],
            'phx_reply',
            { response: {}, status: 'ok' },
          ]));

          resolve([ payload[0], payload[1], payload[2] ]);
        }
      } catch (error) {
        reject(error);
      }
    });
  });
};

export function sendMessage(socket: WebSocket, channel: Channel, msg: 'coin_balance', payload: { coin_balance: AddressCoinBalanceHistoryItem }): void;
export function sendMessage(socket: WebSocket, channel: Channel, msg: 'token_balance', payload: { block_number: number }): void;
export function sendMessage(socket: WebSocket, channel: Channel, msg: `updated_token_balances_${ string }`, payload: AddressTokensBalancesSocketMessage): void;
export function sendMessage(socket: WebSocket, channel: Channel, msg: 'transaction', payload: { transaction: number }): void;
export function sendMessage(socket: WebSocket, channel: Channel, msg: 'transaction', payload: { transactions: Array<Transaction> }): void;
export function sendMessage(socket: WebSocket, channel: Channel, msg: 'pending_transaction', payload: { pending_transaction: number }): void;
export function sendMessage(socket: WebSocket, channel: Channel, msg: 'pending_transaction', payload: { pending_transactions: Array<Transaction> }): void;
export function sendMessage(socket: WebSocket, channel: Channel, msg: 'new_block', payload: NewBlockSocketResponse): void;
export function sendMessage(socket: WebSocket, channel: Channel, msg: 'verification_result', payload: SmartContractVerificationResponse): void;
export function sendMessage(socket: WebSocket, channel: Channel, msg: 'total_supply', payload: { total_supply: number }): void;
export function sendMessage(socket: WebSocket, channel: Channel, msg: 'changed_bytecode', payload: Record<string, never>): void;
export function sendMessage(socket: WebSocket, channel: Channel, msg: 'fetched_bytecode', payload: { fetched_bytecode: string }): void;
export function sendMessage(socket: WebSocket, channel: Channel, msg: 'eth_bytecode_db_lookup_started', payload: Record<string, never>): void;
export function sendMessage(socket: WebSocket, channel: Channel, msg: 'smart_contract_was_verified', payload: Record<string, never>): void;
export function sendMessage(socket: WebSocket, channel: Channel, msg: 'smart_contract_was_not_verified', payload: Record<string, never>): void;
export function sendMessage(socket: WebSocket, channel: Channel, msg: 'token_transfer', payload: { token_transfers: Array<TokenTransfer> }): void;
export function sendMessage(socket: WebSocket, channel: Channel, msg: 'fetched_token_instance_metadata', payload: TokenInstanceMetadataSocketMessage): void;
export function sendMessage(socket: WebSocket, channel: Channel, msg: string, payload: unknown): void {
  socket.send(JSON.stringify([
    ...channel,
    msg,
    payload,
  ]));
}
