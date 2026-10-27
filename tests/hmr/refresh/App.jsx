import { useState } from 'react';

export default function App() {
  const [count, setCount] = useState(0);
  return (
    <div>
      <span id="label">Hello</span> <button onClick={() => setCount(count + 1)}>count {count}</button>
    </div>
  );
}
