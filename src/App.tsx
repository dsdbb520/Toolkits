import { BrowserRouter, Routes, Route, Navigate } from "react-router-dom";
import "./App.css";
import Layout from "@/components/Layout";
import Toolbox from "@/pages/Toolbox";
import Notes from "@/pages/Notes";
import BiliDownload from "@/pages/BiliDownload";
import SteamSwitch from "@/pages/SteamSwitch";
import SteamPrice from "@/pages/SteamPrice";
import Screenshot from "@/pages/Screenshot";
import ImageEditor from "@/pages/ImageEditor";
import MediaEditor from "@/pages/MediaEditor";
import TelegramDownload from "@/pages/TelegramDownload";

export default function App() {
  return (
    <BrowserRouter>
      <Routes>
        <Route path="/" element={<Layout />}>
          <Route index element={<Navigate to="/toolbox" replace />} />
          <Route path="toolbox" element={<Toolbox />} />
          <Route path="toolbox/notes" element={<Notes />} />
          <Route path="toolbox/bili" element={<BiliDownload />} />
          <Route path="toolbox/steam" element={<SteamSwitch />} />
          <Route path="toolbox/steam-price" element={<SteamPrice />} />
          <Route path="toolbox/screenshot" element={<Screenshot />} />
          <Route path="toolbox/image" element={<ImageEditor />} />
          <Route path="toolbox/media" element={<MediaEditor />} />
          <Route path="toolbox/telegram" element={<TelegramDownload />} />
        </Route>
      </Routes>
    </BrowserRouter>
  );
}
