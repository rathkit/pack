import React from 'react';
export const Button = (p: { kind: string; disabled: boolean; children?: React.ReactNode }) => <button className={p.kind} disabled={p.disabled}>{p.children}</button>;
