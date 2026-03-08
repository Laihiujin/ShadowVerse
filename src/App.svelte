<script lang="ts">
  import Room from "./page/Room.svelte";
  import BSidebar from "./lib/components/BSidebar.svelte";
  import Summary from "./page/Summary.svelte";
  import Setting from "./page/Setting.svelte";
  import Account from "./page/Account.svelte";
  import { log, onOpenUrl } from "./lib/invoker";
  import Clip from "./page/Clip.svelte";
  import Task from "./page/Task.svelte";
  import AI from "./page/AI.svelte";
  import Archive from "./page/Archive.svelte";
  import { onMount } from "svelte";

  let active = "总览";
  let darkMode = false;

  function applyTheme(isDark: boolean) {
    darkMode = isDark;
    document.documentElement.classList.toggle("dark", isDark);
  }

  onMount(async () => {
    try {
      log.info("App mounted");
    } catch (e) {
      console.error("Failed to log mount", e);
    }

    const theme = localStorage.getItem("theme");
    const isDark = theme ? theme === "dark" : true;
    applyTheme(isDark);
    if (!theme) {
      localStorage.setItem("theme", "dark");
    }

    await onOpenUrl((urls: string[]) => {
      console.log("Received Deep Link:", urls);
      if (urls.length > 0) {
        const url = urls[0];
        // extract platform and room_id from url
        // url example:
        // bsr://live.bilibili.com/167537?live_from=85001&spm_id_from=333.1365.live_users.item.click
        // bsr://live.douyin.com/200525029536

        let platform = "";
        let room_id = "";

        const bilibiliPrefixes = [
          "bsr://live.bilibili.com/",
          "https://live.bilibili.com/",
          "http://live.bilibili.com/",
        ];
        for (const prefix of bilibiliPrefixes) {
          if (url.startsWith(prefix)) {
            room_id = url.replace(prefix, "").split("?")[0];
            platform = "bilibili";
            break;
          }
        }

        const douyinPrefixes = [
          "bsr://live.douyin.com/",
          "https://live.douyin.com/",
          "http://live.douyin.com/",
        ];
        for (const prefix of douyinPrefixes) {
          if (url.startsWith(prefix)) {
            room_id = url.replace(prefix, "").split("?")[0];
            platform = "douyin";
            break;
          }
        }

        if (url.startsWith("bsr://live.kuaishou.com/")) {
          room_id = url.replace("bsr://live.kuaishou.com/", "").split("?")[0];
          room_id = room_id.replace(/^u\//, "");
          platform = "kuaishou";
        }

        if (url.startsWith("bsr://live.tiktok.com/")) {
          room_id = url.replace("bsr://live.tiktok.com/", "").split("?")[0];
          platform = "tiktok";
        }

        const webcastMatch = url.match(
          /webcast\.tiktok\.com\/webcast\/[^?]+.*[?&]room_id=(\d+)/i
        );
        if (webcastMatch) {
          room_id = webcastMatch[1];
          platform = "tiktok";
        }

        if (
          url.startsWith("https://www.tiktok.com/") ||
          url.startsWith("http://www.tiktok.com/") ||
          url.startsWith("https://tiktok.com/") ||
          url.startsWith("http://tiktok.com/")
        ) {
          const match = url.match(/tiktok\.com\/@?([^\/\?]+)(?:\/live)?/i);
          if (match) {
            room_id = match[1];
            platform = "tiktok";
          }
        }

        // Huya Parsing
        // Supports separators: , ; . ， 。 and mashed urls (e.g. ...com/abchttp...)
        if (!room_id) {
          // Identify Huya URL pattern and extract the room ID (alphanumeric, stops at separator or next http)
          const huyaMatch = url.match(/(?:https?|bsr):\/\/(?:www\.)?huya\.com\/([^?#\s,;，。]+?)(?=https?:\/\/|bsr:\/\/|[,;，。\s]|$)/);
          if (huyaMatch) {
            room_id = huyaMatch[1];
            platform = "huya";
            log.info("Parsed Huya room_id:", room_id);
          }
        }

        if (platform && room_id) {
          // switch to room page
          active = "直播间";
          // TODO: Actually trigger the room load. Currently it just switches tab.
          // Assuming there might be a store or we need to dispatch an event, 
          // but based on existing code, we just restore the file structure first.
        }
      }
    });
  });
</script>

<main>
  <div class="wrap">
    <div class="sidebar">
      <BSidebar
        bind:activeUrl={active}
        on:activeChange={(e) => {
          active = e.detail;
        }}
      />
    </div>
    <div class="content bg-white dark:bg-black">
      <div class="page" class:visible={active == "总览"}>
        <Summary />
      </div>
      <div class="page" class:visible={active == "直播间"}>
        <Room />
      </div>
      <div class="page" class:visible={active == "录播"}>
        <Archive />
      </div>
      <div class="page" class:visible={active == "切片"}>
        <Clip />
      </div>
      <div class="page" class:visible={active == "任务"}>
        <Task />
      </div>
      <div class="page" class:visible={active == "助手"}>
        <AI />
      </div>
      <div class="page" class:visible={active == "账号"}>
        <Account />
      </div>
      <div class="page" class:visible={active == "设置"}>
        <Setting />
      </div>
    </div>
  </div>
</main>

<style>
  .sidebar {
    display: flex;
    height: 100vh;
  }

  .wrap {
    display: flex;
    flex-direction: row;
    height: 100vh;
    overflow: hidden;
    background: #fff;
  }

  :global(.dark) .wrap {
    background: #000;
  }

  .visible {
    opacity: 1 !important;
    height: 100% !important;
    transform: translateX(0) !important;
  }

  .page {
    opacity: 0;
    height: 0;
    transform: translateX(100%);
    overflow: hidden;
    transition:
      opacity 0.5s ease-in-out,
      transform 0.3s ease-in-out;
    display: flex;
    flex-direction: column;
  }

  .content {
    width: 100%;
    height: 100vh;
    overflow: hidden;
  }
</style>
