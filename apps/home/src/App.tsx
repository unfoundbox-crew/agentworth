import { useEffect } from 'react';
import { useHome } from './model/store';
import { gateway } from './ws/client';
import { Spaces } from './shell/Spaces';
import { Floor } from './shell/Floor';
import { Drawer } from './shell/Drawer';

export function App() {
  const current = useHome((s) => s.currentSpace);
  const drawerOpen = useHome((s) => s.drawer.open);
  useEffect(() => {
    if (current) gateway.open(current);
  }, [current]);

  return (
    <div className="h-full grid" style={{ gridTemplateColumns: drawerOpen ? '240px 1fr 420px' : '240px 1fr 0px' }}>
      <Spaces />
      <Floor />
      <Drawer />
    </div>
  );
}
