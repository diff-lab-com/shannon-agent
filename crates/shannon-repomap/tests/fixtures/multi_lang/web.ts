// TypeScript fixture: a small web service module.
// Tests pick this up via the multi-language walk.

export interface User {
  id: number;
  name: string;
}

export class UserService {
  constructor(private readonly apiKey: string) {}

  async fetchUser(id: number): Promise<User> {
    return { id, name: `user-${id}` };
  }

  greet(user: User): string {
    return `Hello, ${user.name}!`;
  }
}

export type ServiceConfig = {
  baseUrl: string;
  timeoutMs: number;
};

export const DEFAULT_CONFIG: ServiceConfig = {
  baseUrl: "https://example.com",
  timeoutMs: 5000,
};

export function buildService(config: ServiceConfig): UserService {
  return new UserService("test");
}
