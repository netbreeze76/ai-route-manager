import importlib.util
import os
import socket
import sys
import tempfile
import unittest
from unittest import mock
from pathlib import Path

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("dpm", HERE / "dynamic_proxy_manager.py")
dpm = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = dpm
SPEC.loader.exec_module(dpm)


def proxy(name):
    return {"name": name, "type": "ss", "server": "127.0.0.1", "port": 1, "cipher": "aes-128-gcm", "password": "x"}


class DynamicSelectionTests(unittest.TestCase):
    def fixed_egress(self):
        return {
            "name": "AI-固定出口",
            "server": "203.0.113.8",
            "port": 8443,
            "cipher": "aes-128-gcm",
            "password": "test-only-password",
        }

    def test_ai_health_check_uses_profile_region_instead_of_global_constant(self):
        self.assertEqual(dpm.AI_TEST_URL, "https://api.openai.com/v1/models")
        self.assertEqual(dpm.AI_EXPECTED_STATUS, 401)
        self.assertEqual(dpm.normalize_region("us"), "US")
        with self.assertRaisesRegex(dpm.ManagerError, "two-letter ISO"):
            dpm.normalize_region("USA")
        self.assertEqual(dpm.trace_region("ip=1.2.3.4\nloc=SG\ncolo=SIN\n"), "SG")
        self.assertEqual(dpm.trace_ip("ip=1.2.3.4\nloc=SG\ncolo=SIN\n"), "1.2.3.4")
        self.assertIn(("chatgpt", dpm.AI_TEST_URL, 401), dpm.PROBES)

    def test_exact_node_preferred_when_present(self):
        stable = dpm.unique_proxy_names([proxy("李家坡2-直連-1G"), proxy("台北")], "稳定")
        config, summary = dpm.build_config(stable, [proxy("下载A")], [proxy("下载B")], dpm.SLOTS[0])
        self.assertEqual(summary["exact_preferred"], 1)
        ai = next(g for g in config["proxy-groups"] if g["name"] == "AI-智能")
        self.assertEqual(ai["proxies"][0], "AI-指定节点")

    def test_removed_exact_node_automatically_rebuilds(self):
        stable = dpm.unique_proxy_names([proxy("新加坡3-直连"), proxy("台北")], "稳定")
        config, summary = dpm.build_config(stable, [proxy("下载A")], [proxy("下载B")], dpm.SLOTS[0])
        self.assertEqual(summary["exact_preferred"], 0)
        self.assertEqual(summary["region_direct"], 1)
        ai = next(g for g in config["proxy-groups"] if g["name"] == "AI-智能")
        self.assertEqual(ai["proxies"][0], "AI-目标地区直连")
        self.assertNotIn("AI-指定节点", ai["proxies"])

    def test_no_stable_target_region_builds_chained_target_region_exit(self):
        stable = dpm.unique_proxy_names([proxy("台北1"), proxy("东京1")], "稳定")
        download = dpm.unique_proxy_names([proxy("新加坡-优化-GPT")], "下载1")
        config, summary = dpm.build_config(stable, download, [], dpm.SLOTS[0])
        self.assertEqual(summary["region_any"], 0)
        self.assertEqual(summary["region_chained"], 1)
        ai = next(g for g in config["proxy-groups"] if g["name"] == "AI-智能")
        self.assertEqual(ai["proxies"], ["AI-目标地区双层"])
        chained = next(p for p in config["proxies"] if p["name"].startswith("[AI目标地区出口]"))
        self.assertEqual(chained["dialer-proxy"], "AI-前置稳定")

    def test_fixed_egress_is_the_only_ai_final_exit(self):
        stable = dpm.unique_proxy_names([proxy("香港4-直連-1G"), proxy("东京1")], "稳定")
        download = dpm.unique_proxy_names([proxy("新加坡-优化-GPT")], "下载1")
        config, summary = dpm.build_config(
            stable,
            download,
            [],
            dpm.SLOTS[0],
            self.fixed_egress(),
        )
        ai = next(g for g in config["proxy-groups"] if g["name"] == "AI-智能")
        self.assertEqual(ai["proxies"], [dpm.FIXED_EGRESS_GROUP])
        fixed_group = next(g for g in config["proxy-groups"] if g["name"] == dpm.FIXED_EGRESS_GROUP)
        self.assertEqual(fixed_group["type"], "fallback")
        self.assertEqual(fixed_group["proxies"][0], "AI-SSH直连固定出口")
        self.assertEqual(fixed_group["proxies"][-1], "AI-固定出口")
        self.assertEqual(fixed_group["interval"], 300)
        fixed = next(p for p in config["proxies"] if p["name"] == "AI-固定出口")
        self.assertEqual(fixed["server"], "203.0.113.8")
        self.assertEqual(fixed["port"], 8443)
        self.assertEqual(fixed["dialer-proxy"], "AI-前置稳定")
        self.assertFalse(fixed["udp"])
        self.assertTrue(summary["fixed_egress"])
        self.assertEqual(summary["fixed_egress_server"], "203.0.113.8")
        self.assertEqual(summary["region_chained"], 0)
        self.assertEqual(summary["stable_frontend_port"], 17901)
        self.assertEqual(summary["full_chain_candidates"], fixed_group["proxies"])
        self.assertFalse(any(g["name"] in {"AI-指定节点", "AI-目标地区直连", "AI-目标地区自动", "AI-目标地区双层"} for g in config["proxy-groups"]))

    def test_fixed_egress_transports_rebuild_from_current_subscription_names(self):
        first = dpm.unique_proxy_names([proxy("台北3-直連-1G"), proxy("东京1")], "稳定")
        second = dpm.unique_proxy_names([proxy("节点已改名-A"), proxy("节点已改名-B")], "稳定")
        config1, _ = dpm.build_config(first, [proxy("下载A")], [], dpm.SLOTS[0], self.fixed_egress())
        config2, _ = dpm.build_config(second, [proxy("下载A")], [], dpm.SLOTS[0], self.fixed_egress())
        dialers1 = {p.get("dialer-proxy") for p in config1["proxies"] if p["name"].startswith("AI-固定出口链-")}
        dialers2 = {p.get("dialer-proxy") for p in config2["proxies"] if p["name"].startswith("AI-固定出口链-")}
        self.assertIn("[稳定] 台北3-直連-1G", dialers1)
        self.assertEqual(dialers2, {"[稳定] 节点已改名-A", "[稳定] 节点已改名-B"})
        self.assertFalse(dialers1 & dialers2)

    def test_stable_frontend_is_preferred_without_changing_backend_ports(self):
        self.assertEqual(dpm.preferred_consumer_port(True, 17898), 17901)
        self.assertEqual(dpm.preferred_consumer_port(True, 17899), 17901)
        self.assertEqual(dpm.preferred_consumer_port(False, 17899), 17899)

    def test_steam_cloud_direct_rules_precede_googleapis_ai_rule(self):
        stable = dpm.unique_proxy_names([proxy("新加坡3-直连")], "稳定")
        config, _ = dpm.build_config(stable, [proxy("下载A")], [], dpm.SLOTS[0], self.fixed_egress())
        rules = config["rules"]
        self.assertLess(rules.index("DOMAIN-SUFFIX,storage.googleapis.com,DIRECT"), rules.index("DOMAIN-SUFFIX,googleapis.com,AI-智能"))

    def test_fixed_egress_validation_rejects_incomplete_settings(self):
        with self.assertRaisesRegex(dpm.ManagerError, "missing required fields"):
            dpm.make_fixed_egress_proxy({"server": "203.0.113.8"}, "AI-前置稳定")

    def test_public_profile_can_target_us_or_hong_kong(self):
        stable = dpm.unique_proxy_names([proxy("洛杉矶2-直连"), proxy("香港4-直連-1G")], "稳定")
        us_config, us_summary = dpm.build_config(stable, [proxy("下载A")], [], dpm.SLOTS[0], None, "US", "美国", ["洛杉矶2"])
        hk_config, hk_summary = dpm.build_config(stable, [proxy("下载A")], [], dpm.SLOTS[0], None, "HK", "香港", [])
        self.assertEqual(us_summary["ai_region"], "US")
        self.assertEqual(hk_summary["ai_region"], "HK")
        self.assertIn("AI-指定节点", next(g for g in us_config["proxy-groups"] if g["name"] == "AI-智能")["proxies"])
        self.assertIn("AI-目标地区直连", next(g for g in hk_config["proxy-groups"] if g["name"] == "AI-智能")["proxies"])

    def test_no_target_region_exit_fails_closed(self):
        stable = dpm.unique_proxy_names([proxy("台北1"), proxy("东京1")], "稳定")
        with self.assertRaisesRegex(dpm.ManagerError, "no SG AI exit"):
            dpm.build_config(stable, [proxy("下载A")], [], dpm.SLOTS[0])

    def test_download_sources_are_automatic(self):
        stable = dpm.unique_proxy_names([proxy("李家坡2-直連-1G")], "稳定")
        config, _ = dpm.build_config(stable, [proxy("下载A")], [proxy("下载B")], dpm.SLOTS[0])
        group = next(g for g in config["proxy-groups"] if g["name"] == "下载线路")
        self.assertEqual(group["type"], "fallback")
        self.assertEqual(group["proxies"], ["下载源1", "下载源2", "AI-前置稳定"])
        self.assertFalse(any(g["type"] == "select" for g in config["proxy-groups"]))

    def test_non_ai_traffic_never_falls_back_to_fixed_ai_egress(self):
        stable = dpm.unique_proxy_names([proxy("李家坡2-直連-1G")], "稳定")
        config, _ = dpm.build_config(stable, [proxy("下载A")], [proxy("下载B")], dpm.SLOTS[0])
        groups = {group["name"]: group for group in config["proxy-groups"]}
        self.assertNotIn("国外其他", groups)
        self.assertNotIn("AI-智能", groups["下载线路"]["proxies"])
        self.assertIn("AI-前置稳定", groups["下载线路"]["proxies"])
        self.assertEqual(config["rules"][-1], "MATCH,下载线路")

    def test_base64_yaml_subscription(self):
        import base64
        raw = b"proxies:\n  - {name: test, type: ss}\n"
        parsed = dpm.decode_subscription(base64.b64encode(raw))
        self.assertEqual(parsed["proxies"][0]["name"], "test")

    def test_slot_ports_do_not_collide_with_flclash(self):
        self.assertTrue(all(s["proxy_port"] not in (7890, 17896) for s in dpm.SLOTS))
        self.assertTrue(all(s["controller_port"] not in (7890, 17896, 19096) for s in dpm.SLOTS))

    @unittest.skipUnless(os.name == "nt", "Windows listener ownership check")
    def test_listener_owner_prevents_stale_pid_retirement(self):
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
            listener.bind(("127.0.0.1", 0))
            listener.listen(1)
            port = listener.getsockname()[1]
            self.assertEqual(dpm.listener_owner(port), os.getpid())

    def test_preview_always_stops_candidate(self):
        with tempfile.TemporaryDirectory() as root:
            home = Path(root) / "home"
            app = Path(root) / "app"
            app.mkdir()
            manager = dpm.DynamicProxyManager(home, app, app / "subscriptions.json")
            slot = dpm.SLOTS[0]
            config = home / "slot-A" / "config.yaml"
            summary = {"fixed_egress_server": "203.0.113.8"}
            with (
                mock.patch.object(manager, "prepare_candidate", return_value=(slot, config, summary)),
                mock.patch.object(manager, "syntax_test"),
                mock.patch.object(manager, "start_candidate", return_value=4242),
                mock.patch.object(manager, "select_ai_exit", return_value={"name": "fixed"}),
                mock.patch.object(manager, "probe", return_value={"chatgpt": 401}),
                mock.patch.object(manager, "group_state", return_value={"AI-智能": "fixed"}),
                mock.patch.object(manager, "stop_candidate") as stop,
            ):
                result = manager.preview()
            self.assertTrue(result["candidate_stopped"])
            stop.assert_called_once_with(4242, slot)

    def test_preview_stops_candidate_when_probe_fails(self):
        with tempfile.TemporaryDirectory() as root:
            home = Path(root) / "home"
            app = Path(root) / "app"
            app.mkdir()
            manager = dpm.DynamicProxyManager(home, app, app / "subscriptions.json")
            slot = dpm.SLOTS[0]
            config = home / "slot-A" / "config.yaml"
            with (
                mock.patch.object(manager, "prepare_candidate", return_value=(slot, config, {})),
                mock.patch.object(manager, "syntax_test"),
                mock.patch.object(manager, "start_candidate", return_value=4243),
                mock.patch.object(manager, "select_ai_exit", side_effect=dpm.ManagerError("probe failed")),
                mock.patch.object(manager, "stop_candidate") as stop,
            ):
                with self.assertRaisesRegex(dpm.ManagerError, "probe failed"):
                    manager.preview()
            stop.assert_called_once_with(4243, slot)

    def test_profile_can_remain_in_memory_without_secret_files(self):
        with tempfile.TemporaryDirectory() as root:
            app = Path(root) / "app"
            app.mkdir()
            profile = {
                "subscriptions": [{"role": "stable", "url": "https://example.com/token"}],
                "fixed_egress": self.fixed_egress(),
            }
            manager = dpm.DynamicProxyManager(Path(root) / "home", app, app / "missing.json", profile=profile)
            self.assertEqual(manager.subscription_settings()["subscriptions"], profile["subscriptions"])
            self.assertEqual(manager.fixed_egress_settings(), profile["fixed_egress"])
            self.assertFalse((app / "missing.json").exists())

    def test_run_lock_rejects_a_second_manager_for_the_same_home(self):
        with tempfile.TemporaryDirectory() as root:
            home = Path(root) / "home"
            with dpm.RunLock(home):
                with self.assertRaisesRegex(dpm.ManagerError, "already running"):
                    with dpm.RunLock(home):
                        pass

    def test_run_loop_records_and_clears_manager_pid(self):
        with tempfile.TemporaryDirectory() as root:
            app = Path(root) / "app"
            app.mkdir()
            manager = dpm.DynamicProxyManager(Path(root) / "home", app, app / "subscriptions.json")
            with mock.patch.object(manager, "_run_loop") as inner:
                manager.run_loop(300)
            inner.assert_called_once_with(300)
            state = manager.state()
            self.assertNotIn("manager_pid", state)
            self.assertIn("manager_stopped_at", state)

    def test_stop_rolls_back_before_removing_runtime_configs(self):
        with tempfile.TemporaryDirectory() as root:
            home = Path(root) / "home"
            app = Path(root) / "app"
            app.mkdir()
            manager = dpm.DynamicProxyManager(home, app, app / "subscriptions.json")
            for slot in dpm.SLOTS:
                config = home / f"slot-{slot['name']}" / "config.yaml"
                config.parent.mkdir(parents=True, exist_ok=True)
                config.write_text("password: secret", encoding="utf-8")
            manager.save_state({"active_slot": "A", "active_pid": 123, "active_proxy_port": dpm.SLOTS[0]["proxy_port"]})
            with (
                mock.patch.object(manager, "rollback") as rollback,
                mock.patch.object(manager, "_stop_recorded_cores", return_value=[123]),
                mock.patch.object(manager, "current_wininet_proxy", return_value={"ProxyServer": "127.0.0.1:7890"}),
            ):
                result = manager.stop_and_rollback()
            rollback.assert_called_once_with()
            self.assertTrue(result["rollback"])
            self.assertEqual(result["stopped_core_pids"], [123])
            self.assertTrue(all(not (home / f"slot-{slot['name']}" / "config.yaml").exists() for slot in dpm.SLOTS))
            self.assertNotIn("active_pid", manager.state())

    def test_macos_proxy_snapshot_preserves_each_service(self):
        with (
            mock.patch.object(dpm.sys, "platform", "darwin"),
            mock.patch.object(dpm, "mac_network_services", return_value=["Wi-Fi", "USB LAN"]),
            mock.patch.object(
                dpm,
                "mac_proxy_details",
                side_effect=[
                    {"enabled": False, "server": "", "port": 0},
                    {"enabled": True, "server": "127.0.0.1", "port": 7890},
                    {"enabled": False, "server": "", "port": 0},
                    {"enabled": False, "server": "", "port": 0},
                ],
            ),
        ):
            snapshot = dpm.DynamicProxyManager.current_wininet_proxy()
        self.assertEqual(snapshot["ProxyEnable"], 1)
        self.assertEqual(snapshot["ProxyServer"], "127.0.0.1:7890")
        self.assertEqual([item["name"] for item in snapshot["mac_services"]], ["Wi-Fi", "USB LAN"])

    def test_macos_proxy_apply_and_restore_are_service_exact(self):
        original = {
            "ProxyEnable": 1,
            "ProxyServer": "127.0.0.1:7890",
            "mac_restore_services": True,
            "mac_services": [
                {
                    "name": "Wi-Fi",
                    "web": {"enabled": False, "server": "", "port": 0},
                    "secure": {"enabled": True, "server": "127.0.0.1", "port": 7890},
                },
            ],
        }
        with mock.patch.object(dpm.sys, "platform", "darwin"), mock.patch.object(dpm, "set_mac_proxy") as setter:
            dpm.DynamicProxyManager.set_wininet_proxy(
                {"ProxyEnable": 1, "ProxyServer": "127.0.0.1:27898", "mac_services": [{"name": "Wi-Fi"}]}
            )
            self.assertEqual(
                setter.call_args_list,
                [
                    mock.call("Wi-Fi", False, True, "127.0.0.1", 27898),
                    mock.call("Wi-Fi", True, True, "127.0.0.1", 27898),
                ],
            )
            setter.reset_mock()
            dpm.DynamicProxyManager.set_wininet_proxy(original)
            self.assertEqual(
                setter.call_args_list,
                [
                    mock.call("Wi-Fi", False, False, "", 0),
                    mock.call("Wi-Fi", True, True, "127.0.0.1", 7890),
                ],
            )


if __name__ == "__main__":
    unittest.main()
