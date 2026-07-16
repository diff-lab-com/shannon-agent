import { Outlet } from 'react-router-dom';

// The left Sidebar already exposes the Settings subcategories
// (General / Theme / Models / Usage & Billing / Advanced) for both desktop
// and the mobile overlay, so this page only needs to render the active pane.
export default function Settings() {
  return (
    <div className="flex-1 overflow-y-auto h-full w-full bg-background pb-8">
      <div className="max-w-[1000px] mx-auto px-lg py-xl animate-in fade-in duration-700">
        <Outlet />
      </div>
    </div>
  );
}
