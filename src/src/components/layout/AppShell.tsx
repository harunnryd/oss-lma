import { Outlet, NavLink, useLocation } from 'react-router-dom';
import { LayoutDashboard, Radio, Settings, History as HistoryIcon, Sparkles, ShieldCheck } from 'lucide-react';

import { cn } from '@/lib/utils';
import { Separator } from '@/components/ui/separator';
import { Badge } from '@/components/ui/badge';
import { useMeetingEvents } from '@/hooks/useMeetingEvents';
import { useMeetingStore } from '@/stores/meetingStore';

const NAV = [
  { to: '/', label: 'Dashboard', icon: LayoutDashboard, end: true },
  { to: '/live', label: 'Live meeting', icon: Radio },
  { to: '/history', label: 'History', icon: HistoryIcon },
  { to: '/settings', label: 'Settings', icon: Settings },
];

export function AppShell() {
  useMeetingEvents();
  const phase = useMeetingStore((s) => s.phase);
  const meetingId = useMeetingStore((s) => s.meetingId);
  const location = useLocation();

  return (
    <div className="flex min-h-screen w-full">
        <aside className="sticky top-0 flex h-screen w-60 shrink-0 flex-col border-r border-sidebar-border bg-sidebar text-sidebar-foreground">
          <div className="flex h-14 items-center gap-2 border-b border-sidebar-border px-4">
            <div className="grid h-8 w-8 place-items-center rounded-md bg-primary/15 text-primary">
              <Sparkles className="h-4 w-4" />
            </div>
            <div className="flex flex-col leading-tight">
              <span className="text-sm font-semibold text-foreground">oss-lma</span>
              <span className="text-xs text-muted-foreground">private capture</span>
            </div>
          </div>

          <nav className="flex flex-1 flex-col gap-1 p-3">
            {NAV.map(({ to, label, icon: Icon, end }) => (
              <NavLink
                key={to}
                to={to}
                end={end}
                className={({ isActive }) =>
                  cn(
                    'flex items-center gap-3 rounded-md px-3 py-2 text-sm transition-colors',
                    isActive
                      ? 'bg-sidebar-accent text-foreground'
                      : 'text-sidebar-foreground hover:bg-sidebar-accent hover:text-foreground',
                  )
                }
              >
                <Icon className="h-4 w-4" />
                {label}
              </NavLink>
            ))}
          </nav>

          <div className="border-t border-sidebar-border p-3">
            <div className="flex items-center gap-2 rounded-md bg-sidebar-accent/40 px-3 py-2">
              <ShieldCheck className="h-4 w-4 text-emerald-400" />
              <div className="flex flex-1 flex-col text-xs leading-tight">
                <span className="font-medium text-foreground">Local-first</span>
                <span className="text-muted-foreground">Secrets stay in keychain</span>
              </div>
            </div>
          </div>
        </aside>

        <main className="flex min-h-screen flex-1 flex-col">
          <header className="sticky top-0 z-30 flex h-14 items-center justify-between border-b border-border bg-background/85 px-6 backdrop-blur">
            <div className="flex items-center gap-3 text-sm text-muted-foreground">
              <span className="font-medium text-foreground">{pageTitle(location.pathname)}</span>
            </div>
            <div className="flex items-center gap-2">
              <PhaseIndicator phase={phase} meetingId={meetingId} />
            </div>
          </header>
          <div className="flex-1 px-6 py-6">
            <Outlet />
          </div>
        </main>
    </div>
  );
}

function pageTitle(pathname: string) {
  if (pathname === '/') return 'Dashboard';
  if (pathname.startsWith('/onboarding')) return 'Onboarding';
  if (pathname.startsWith('/live')) return 'Live meeting';
  if (pathname.startsWith('/settings')) return 'Settings';
  if (pathname.startsWith('/history')) return 'History';
  return 'oss-lma';
}

function PhaseIndicator({ phase, meetingId }: { phase: string; meetingId: string | null }) {
  const variant = phase === 'Active' ? 'success' : phase === 'Failed' ? 'destructive' : phase === 'Paused' ? 'warning' : 'secondary';
  const label = phase === 'Idle' ? 'Idle' : phase;
  return (
    <Badge variant={variant as never} className="gap-1.5">
      <span
        className={cn(
          'inline-block h-1.5 w-1.5 rounded-full',
          phase === 'Active' && 'animate-pulse bg-emerald-400',
          phase === 'Failed' && 'bg-red-400',
          phase === 'Paused' && 'bg-amber-400',
          phase === 'Idle' && 'bg-muted-foreground/60',
        )}
      />
      <span className="font-medium">{label}</span>
      {meetingId && (
        <>
          <Separator orientation="vertical" className="mx-1 h-3" />
          <span className="font-mono text-xs text-muted-foreground">{meetingId.slice(0, 8)}</span>
        </>
      )}
    </Badge>
  );
}
