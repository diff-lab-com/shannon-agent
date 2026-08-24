// Platform + secret-slot constants shared by ConnectionsSettings and its
// sub-components. No React state, no side effects — kept here so individual
// sub-components can import them without dragging the orchestrator along.

// The gateway platforms (mirrors the `Platform` enum in
// shannon-gateway/src/adapters/types.ts). Order = display order.
export const PLATFORMS = [
  'slack',
  'telegram',
  'discord',
  'matrix',
  'whatsapp',
  'wecom',
  'feishu',
  'dingtalk',
] as const
export type Platform = (typeof PLATFORMS)[number]

export const PLATFORM_LABEL: Record<Platform, string> = {
  slack: 'Slack',
  telegram: 'Telegram',
  discord: 'Discord',
  matrix: 'Matrix',
  whatsapp: 'WhatsApp',
  wecom: 'WeCom (企业微信)',
  feishu: 'Feishu (飞书)',
  dingtalk: 'DingTalk (钉钉)',
}

// One credential slot the gateway reads from the OS keyring at adapter
// start(). The `name` is the adapter-local key (it becomes the entry in
// GatewayAdapter.secrets), and `key` is the exact OS-keyring key the
// gateway's ctx.getSecret(...) call reads. These keys are verified against
// each adapter's start() in shannon-gateway/src/adapters/*.
//
// `required` mirrors whether the adapter throws when the slot is missing:
// a platform is "connected" once every required slot has a stored value.
export interface SecretSlot {
  name: string
  key: string
  labelKey: string
  required: boolean
}

export const slot = (name: string, key: string, labelKey: string, required: boolean): SecretSlot => ({
  name,
  key,
  labelKey,
  required,
})

const S = (n: string) => `settings.connections.secret.${n}.label`

export const SECRET_MODEL: Record<Platform, SecretSlot[]> = {
  slack: [
    slot('botToken', 'slack/bot-token', S('botToken'), true),
    slot('signingSecret', 'slack/signing-secret', S('signingSecret'), true),
  ],
  telegram: [slot('botToken', 'telegram/bot-token', S('botToken'), true)],
  discord: [slot('botToken', 'discord/bot-token', S('botToken'), true)],
  matrix: [slot('accessToken', 'matrix/access-token', S('accessToken'), true)],
  whatsapp: [
    slot('accessToken', 'whatsapp/access-token', S('accessToken'), true),
    slot('appSecret', 'whatsapp/app-secret', S('appSecret'), false),
  ],
  wecom: [
    slot('corpSecret', 'wecom/corp-secret', S('corpSecret'), true),
    slot('encodingAesKey', 'wecom/encoding-aes-key', S('encodingAesKey'), true),
  ],
  feishu: [
    slot('appSecret', 'feishu/app-secret', S('appSecret'), true),
    slot('encryptKey', 'feishu/encrypt-key', S('encryptKey'), false),
  ],
  dingtalk: [slot('robotSecret', 'dingtalk/robot-secret', S('robotSecret'), true)],
}

// Flatten once for the keyring-presence probe.
export const ALL_SLOTS = PLATFORMS.flatMap((p) => SECRET_MODEL[p].map((s) => ({ p, s })))
