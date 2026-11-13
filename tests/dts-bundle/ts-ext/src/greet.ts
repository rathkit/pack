export interface Greeting { text: string }
export function greet(name: string): Greeting {
  return { text: 'hi ' + name };
}
