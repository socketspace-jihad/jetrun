import { Nav } from "@/components/nav";

export default function AppLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <div className="flex h-screen">
      <Nav />
      <main className="flex-1 overflow-y-auto custom-scrollbar">
        {children}
      </main>
    </div>
  );
}
