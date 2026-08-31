import http from './http';

export interface SshKeyInfo {
  /// Ключ настроен админом (env AGA_SSH_PRIVATE_KEY).
  configured: boolean;
  /// Публичный ключ в OpenSSH-формате (ssh-ed25519 AAAA...).
  public_key: string | null;
  /// SHA256-fingerprint (SHA256:...).
  fingerprint: string | null;
}

/// Публичный SSH-ключ aga (страница «Настройки»). Приватный ключ в env ядра
/// задаёт админ — здесь отдаётся только публичный.
export const getSshKey = async (): Promise<SshKeyInfo> => {
  const { data } = await http.get('/settings/ssh-key');
  return data;
};