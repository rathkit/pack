import React, { useState } from 'react';
// Hooks fail with two copies of React, and a second copy of react-dom would not render this tree.
export default function App({ label }) {
  const [n] = useState(41);
  return React.createElement('span', null, `${label} ${n + 1} react ${React.version}`);
}
