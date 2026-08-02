// Fixture: tiny_ts.ts
// Smallest possible TypeScript file with multiple top-level declarations
// covering function / class / interface / type alias / const.

export function add(a: number, b: number): number {
    return a + b;
}

export interface Greeter {
    name: string;
    greet(): string;
}

export type Pair<T> = { left: T; right: T };

export class Counter implements Greeter {
    public name: string = "counter";
    private count: number = 0;

    constructor(initial: number) {
        this.count = initial;
    }

    public increment(): void {
        this.count += 1;
    }

    public greet(): string {
        return `hello, ${this.name}`;
    }
}

export const DEFAULT_LIMIT = 100;
