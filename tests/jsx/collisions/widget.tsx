import React from 'react';
export const Widget = ({ size }: { size: number }) => <div style={{ width: size }}>{[1, 2].map((n) => <i key={n}>{n * size}</i>)}</div>;
const App = () => <p>shadowed name in another module</p>;
export { App as UnusedApp };
