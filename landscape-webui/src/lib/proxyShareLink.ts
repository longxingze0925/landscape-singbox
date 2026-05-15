import type {
  ProxyNodeConfig,
  ProxyProtocolConfig,
} from "@landscape-router/types/api/schemas";

export type ParsedProxyShareLink = {
  raw: string;
  node?: ProxyNodeConfig;
  error?: string;
};

type VmessPayload = {
  ps?: string;
  add?: string;
  port?: string | number;
  id?: string;
  aid?: string | number;
  scy?: string;
  tls?: string;
  sni?: string;
  host?: string;
};

export function parseProxyShareLinks(input: string): ParsedProxyShareLink[] {
  return input
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .map(parseProxyShareLink);
}

function parseProxyShareLink(raw: string): ParsedProxyShareLink {
  try {
    const scheme = raw.slice(0, raw.indexOf(":")).toLowerCase();
    switch (scheme) {
      case "vless":
        return { raw, node: parseVless(raw) };
      case "vmess":
        return { raw, node: parseVmess(raw) };
      case "ss":
        return { raw, node: parseShadowsocks(raw) };
      case "socks":
      case "socks5":
        return { raw, node: parseSocks(raw) };
      default:
        return { raw, error: `Unsupported scheme: ${scheme || "-"}` };
    }
  } catch (err) {
    return {
      raw,
      error: err instanceof Error ? err.message : String(err),
    };
  }
}

function parseVless(raw: string): ProxyNodeConfig {
  const url = new URL(raw);
  const security = url.searchParams.get("security")?.toLowerCase();
  const reality = security === "reality";
  const protocol: ProxyProtocolConfig = {
    t: "vless",
    uuid: decodeURIComponent(url.username),
    flow: optional(url.searchParams.get("flow")),
    tls: reality || url.searchParams.get("security") === "tls",
    server_name: optional(
      url.searchParams.get("sni") || url.searchParams.get("host"),
    ),
    reality,
    reality_public_key: optional(url.searchParams.get("pbk")),
    reality_short_id: optional(url.searchParams.get("sid")),
    utls_fingerprint: optional(url.searchParams.get("fp")),
  };

  return nodeConfig({
    name: decodeName(url.hash, url.hostname),
    server: url.hostname,
    port: requiredPort(url.port),
    protocol,
    remark: "",
  });
}

function parseVmess(raw: string): ProxyNodeConfig {
  const payload = JSON.parse(
    decodeBase64(raw.replace(/^vmess:\/\//i, "")),
  ) as VmessPayload;
  const protocol: ProxyProtocolConfig = {
    t: "vmess",
    uuid: required(payload.id, "vmess uuid"),
    alter_id: Number(payload.aid ?? 0),
    security: optional(payload.scy || "auto"),
    tls: payload.tls === "tls",
    server_name: optional(payload.sni || payload.host),
  };

  return nodeConfig({
    name: payload.ps || payload.add || "VMess",
    server: required(payload.add, "vmess server"),
    port: requiredPort(String(payload.port ?? "")),
    protocol,
    remark: "",
  });
}

function parseShadowsocks(raw: string): ProxyNodeConfig {
  const body = raw.replace(/^ss:\/\//i, "");
  const [main, hash = ""] = body.split("#", 2);
  const decodedName = hash ? decodeURIComponent(hash) : "Shadowsocks";
  const decodedMain = main.includes("@") ? main : decodeBase64(main);
  const at = decodedMain.lastIndexOf("@");
  if (at < 0) throw new Error("Invalid Shadowsocks link");

  const userInfo = decodedMain.slice(0, at);
  const serverInfo = decodedMain.slice(at + 1);
  const [method, password] = decodeMaybeBase64(userInfo).split(":", 2);
  const { host, port } = splitHostPort(serverInfo);

  return nodeConfig({
    name: decodedName,
    server: host,
    port,
    protocol: {
      t: "shadowsocks",
      method: required(method, "ss method"),
      password: required(password, "ss password"),
    },
    remark: "",
  });
}

function parseSocks(raw: string): ProxyNodeConfig {
  const url = new URL(raw.replace(/^socks:\/\//i, "socks5://"));
  return nodeConfig({
    name: decodeName(url.hash, url.hostname),
    server: url.hostname,
    port: requiredPort(url.port),
    protocol: {
      t: "socks5",
      username: optional(decodeURIComponent(url.username)),
      password: optional(decodeURIComponent(url.password)),
    },
    remark: "",
  });
}

function nodeConfig(input: {
  name: string;
  server: string;
  port: number;
  protocol: ProxyProtocolConfig;
  remark: string;
}): ProxyNodeConfig {
  return {
    enable: true,
    name: input.name || input.server,
    server: input.server,
    port: input.port,
    protocol: input.protocol,
    remark: input.remark,
  };
}

function decodeName(hash: string, fallback: string): string {
  return hash ? decodeURIComponent(hash.slice(1)) : fallback;
}

function decodeBase64(value: string): string {
  const normalized = value.replace(/-/g, "+").replace(/_/g, "/");
  const padded = normalized.padEnd(
    normalized.length + ((4 - (normalized.length % 4)) % 4),
    "=",
  );
  return decodeURIComponent(
    Array.from(atob(padded))
      .map((char) => `%${char.charCodeAt(0).toString(16).padStart(2, "0")}`)
      .join(""),
  );
}

function decodeMaybeBase64(value: string): string {
  if (value.includes(":")) return value;
  try {
    return decodeBase64(value);
  } catch {
    return value;
  }
}

function splitHostPort(value: string): { host: string; port: number } {
  const url = new URL(`tcp://${value}`);
  return { host: url.hostname, port: requiredPort(url.port) };
}

function required<T>(value: T | null | undefined, label: string): T {
  if (value === null || value === undefined || value === "") {
    throw new Error(`Missing ${label}`);
  }
  return value;
}

function requiredPort(value: string): number {
  const port = Number(value);
  if (!Number.isInteger(port) || port < 1 || port > 65535) {
    throw new Error("Invalid port");
  }
  return port;
}

function optional(value: string | null | undefined): string | null {
  const trimmed = value?.trim();
  return trimmed ? trimmed : null;
}
