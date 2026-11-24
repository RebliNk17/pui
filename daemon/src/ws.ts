import crypto from 'node:crypto';
import type http from 'node:http';
import type { Duplex } from 'node:stream';

const WS_GUID = '258EAFA5-E914-47DA-95CA-C5AB0DC85B11';

export interface WsClient {
  id: string;
  socket: Duplex;
  subscribedPath: string | null;
  device: string;
  platform: string;
  sendJson: (payload: unknown) => void;
  close: () => void;
}

export function encodeWsFrame(opcode: number, payload: Buffer): Buffer {
  const len = payload.length;
  let header: Buffer;
  if (len < 126) {
    header = Buffer.from([0x80 | opcode, len]);
  } else if (len < 65536) {
    header = Buffer.alloc(4);
    header[0] = 0x80 | opcode;
    header[1] = 126;
    header.writeUInt16BE(len, 2);
  } else {
    header = Buffer.alloc(10);
    header[0] = 0x80 | opcode;
    header[1] = 127;
    header.writeBigUInt64BE(BigInt(len), 2);
  }
  return Buffer.concat([header, payload]);
}

export function encodeWsText(text: string): Buffer {
  return encodeWsFrame(0x1, Buffer.from(text, 'utf8'));
}

export function parseWsFrames(
  buf: Buffer,
): { messages: { opcode: number; payload: Buffer }[]; remainder: Buffer } {
  const messages: { opcode: number; payload: Buffer }[] = [];
  let offset = 0;

  while (offset + 2 <= buf.length) {
    const b0 = buf[offset];
    const b1 = buf[offset + 1];
    const opcode = b0 & 0x0f;
    const masked = (b1 & 0x80) !== 0;
    let payloadLen = b1 & 0x7f;
    let pos = offset + 2;

    if (payloadLen === 126) {
      if (pos + 2 > buf.length) break;
      payloadLen = buf.readUInt16BE(pos);
      pos += 2;
    } else if (payloadLen === 127) {
      if (pos + 8 > buf.length) break;
      const big = buf.readBigUInt64BE(pos);
      if (big > BigInt(4 * 1024 * 1024)) {
        return { messages, remainder: Buffer.alloc(0) };
      }
      payloadLen = Number(big);
      pos += 8;
    }

    const maskLen = masked ? 4 : 0;
    if (pos + maskLen + payloadLen > buf.length) break;

    let payload: Buffer;
    if (masked) {
      const mask = buf.subarray(pos, pos + 4);
      const raw = buf.subarray(pos + 4, pos + 4 + payloadLen);
      payload = Buffer.alloc(payloadLen);
      for (let i = 0; i < payloadLen; i += 1) {
        payload[i] = raw[i] ^ mask[i & 3];
      }
    } else {
      payload = Buffer.from(buf.subarray(pos, pos + payloadLen));
    }
    messages.push({ opcode, payload });
    offset = pos + maskLen + payloadLen;
  }

  return { messages, remainder: buf.subarray(offset) };
}

export function acceptWebSocket(
  req: http.IncomingMessage,
  socket: Duplex,
  head: Buffer,
  onClientMessage: (client: WsClient, msg: Record<string, unknown>) => void,
  onClose: (client: WsClient) => void,
): WsClient | null {
  const key = req.headers['sec-websocket-key'];
  if (typeof key !== 'string' || !key) {
    socket.write('HTTP/1.1 400 Bad Request\r\n\r\n');
    socket.destroy();
    return null;
  }

  const accept = crypto.createHash('sha1').update(key + WS_GUID).digest('base64');
  socket.write(
    [
      'HTTP/1.1 101 Switching Protocols',
      'Upgrade: websocket',
      'Connection: Upgrade',
      `Sec-WebSocket-Accept: ${accept}`,
      '',
      '',
    ].join('\r\n'),
  );

  const client: WsClient = {
    id: crypto.randomUUID(),
    socket,
    subscribedPath: null,
    device: 'Pui Mobile',
    platform: 'android',
    sendJson(payload: unknown) {
      if (socket.destroyed) return;
      try {
        socket.write(encodeWsText(JSON.stringify(payload)));
      } catch {
        // ignore write errors on closing sockets
      }
    },
    close() {
      if (!socket.destroyed) {
        try {
          socket.write(encodeWsFrame(0x8, Buffer.alloc(0)));
        } catch {
          // ignore
        }
        socket.end();
      }
    },
  };

  let pending: Buffer = head.length > 0 ? Buffer.from(head) : Buffer.alloc(0);

  const processChunk = (chunk: Buffer) => {
    pending = pending.length > 0 ? Buffer.concat([pending, chunk]) : chunk;
    const { messages, remainder } = parseWsFrames(pending);
    pending = Buffer.from(remainder);

    for (const frame of messages) {
      if (frame.opcode === 0x8) {
        client.close();
        return;
      }
      if (frame.opcode === 0x9) {
        // Ping -> Pong
        if (!socket.destroyed) socket.write(encodeWsFrame(0xa, frame.payload));
        continue;
      }
      if (frame.opcode === 0x1) {
        try {
          const parsed = JSON.parse(frame.payload.toString('utf8')) as Record<string, unknown>;
          onClientMessage(client, parsed);
        } catch {
          // ignore malformed client frames
        }
      }
    }
  };

  if (pending.length > 0) {
    const initial = pending;
    pending = Buffer.alloc(0);
    processChunk(initial);
  }

  socket.on('data', (chunk: Buffer) => processChunk(chunk));
  socket.on('close', () => onClose(client));
  socket.on('error', () => onClose(client));

  return client;
}
