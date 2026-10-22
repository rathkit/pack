import React from 'react';
export const jsx = 'inner-jsx-name';
export const App = ({ label }: { label: string }) => <div className="inner">{label}{jsx}</div>;
const Widget = () => <span>unused local</span>;
