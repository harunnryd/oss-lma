import { create } from 'zustand';

import type { PermissionSnapshot } from '@/lib/tauri';

type PermissionState = {
  permissions: PermissionSnapshot;
  setPermissions: (perms: PermissionSnapshot) => void;
};

export const usePermissionStore = create<PermissionState>((set) => ({
  permissions: { screenRecording: 'Unknown', microphone: 'Unknown' },
  setPermissions: (perms) => set({ permissions: perms }),
}));
