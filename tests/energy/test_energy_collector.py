import json, math, os, sys, tempfile, unittest
from pathlib import Path
from dataclasses import replace
from unittest.mock import patch
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "tools/energy"))
from config import Config
from model import Counter, Gpu, Reading
from intervals import Intervals
from history import History
from report import Report
from sensors import Sensors

def reading(at=1000, joules=100, cpu=1000, boot="boot"):
    return Reading(at, at, boot, [Gpu(str(i), "GPU "+str(i), 10+i, joules*(i+1)) for i in range(3)],
                   {"package-0":Counter(cpu,65000)})

class EnergyTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory(); self.config=Config(self.temp.name)
    def tearDown(self):
        self.temp.cleanup()
    def batch(self):
        return Intervals(self.config).between((reading(),reading(1030,400,1600)))
    def test_counter_energy_and_total_assumptions(self):
        batch=self.batch(); items={x.id:x for x in batch.intervals}
        self.assertAlmostEqual(items["0"].kwh,300/3600000)
        self.assertAlmostEqual(items["cpu-package"].kwh,600/3600000)
        self.assertAlmostEqual(items["estimated-total"].kwh,(1800+600+50*30)/.85/3600000)
        self.assertEqual(items["0"].kind,"measured_device")
    def test_unavailable_cpu_and_missing_gpu_do_not_invent_total(self):
        for now in [replace(reading(1030,400,1600),packages={}),
                    replace(reading(1030,400,1600),gpus=reading(1030,400,1600).gpus[:2])]:
            batch=Intervals(self.config).between((reading(),now))
            self.assertNotIn("estimated-total",{i.id for i in batch.intervals})
            self.assertTrue(batch.issues)
    def test_gpu_reset_omits_only_affected_interval(self):
        now=reading(1030,400,1600); now=replace(now,gpus=[replace(now.gpus[0],joules=1),*now.gpus[1:]])
        batch=Intervals(self.config).between((reading(),now))
        self.assertNotIn("0",{i.id for i in batch.intervals})
        self.assertNotIn("estimated-total",{i.id for i in batch.intervals})
        self.assertIn("1",{i.id for i in batch.intervals})
    def test_boot_large_gap_and_clock_change_are_gaps(self):
        for now in [reading(1030,400,1600,boot="new"),reading(1200,400,1600),
                    replace(reading(1030,400,1600),monotonic=1050)]:
            self.assertEqual(Intervals(self.config).between((reading(),now)).intervals,[])
    def test_package_single_wrap_and_ambiguous_wrap(self):
        old=reading(cpu=64900); now=reading(1030,400,200)
        self.assertEqual(Intervals.cpu_delta((old,now)),300)
        self.assertIsNone(Intervals.cpu_delta((old,replace(now,monotonic=1090))))
    def test_cpu_subzones_are_not_double_counted(self):
        root=Path(self.temp.name); package=root/"p"; core=root/"c"
        for p,name in [(package,"package-0"),(core,"core")]:
            p.mkdir();(p/"name").write_text(name);(p/"energy_uj").write_text("10000000");(p/"max_energy_range_uj").write_text("65000000000")
        class Source:
            def read(self):return []
        (root/"control-type").mkdir()
        actual=Path
        def mapped(path):
            return root if path=="/sys/class/powercap" else actual(path)
        sensors=Sensors(Source())
        with patch("sensors.Path",side_effect=mapped):
            sample=sensors.read()
        self.assertEqual(set(sample.packages),{"package-0"})
        self.assertNotIn("CPU package energy unavailable",sample.issues)
    def test_history_restart_is_durable_and_duplicate_rejected(self):
        first=History(self.config);first.record(self.batch())
        expected=first.db.execute("SELECT SUM(kwh) FROM buckets WHERE size=60").fetchone()[0]
        checkpoint=json.loads(first.meta("checkpoint"));first.db.close()
        second=History(self.config)
        self.assertEqual(Reading.decode(checkpoint),self.batch().reading)
        self.assertAlmostEqual(second.db.execute("SELECT SUM(kwh) FROM buckets WHERE size=60").fetchone()[0],expected)
        with self.assertRaises(ValueError):second.record(self.batch())
        second.db.close()
    def test_rollups_preserve_energy_across_boundaries(self):
        batch=Intervals(self.config).between((reading(3590),reading(3620,400,1600)))
        history=History(self.config);history.record(batch)
        expected=sum(i.kwh for i in batch.intervals)
        for size in [60,3600,86400]:
            value=history.db.execute("SELECT SUM(kwh) FROM buckets WHERE size=?",(size,)).fetchone()[0]
            self.assertAlmostEqual(value,expected)
        history.db.close()
    def test_report_atomic_public_fields_and_coverage(self):
        history=History(self.config);batch=self.batch();history.record(batch);Report(history).publish(batch)
        path=Path(self.temp.name)/"public.json";data=json.loads(path.read_text())
        self.assertEqual(path.stat().st_mode&0o777,0o640)
        self.assertNotIn("boot",data);self.assertNotIn("checkpoint",data);self.assertNotIn("packages",data)
        for window in data["history"].values():
            for series in window["series"]:
                self.assertEqual(series["coverage_seconds"],30)
        self.assertFalse(data["methodology"]["calibrated"])
        history.db.close()
    def test_retention_bounded_and_methodology_frozen(self):
        history=History(self.config)
        history.db.execute("INSERT INTO buckets VALUES(60,-999999999,'old',1,30)");history.db.commit()
        history.record(self.batch())
        self.assertEqual(history.db.execute("SELECT COUNT(*) FROM buckets WHERE series='old'").fetchone()[0],0)
        self.assertEqual(history.db.execute("PRAGMA max_page_count").fetchone()[0],32768)
        history.db.close()
        with self.assertRaises(ValueError):History(replace(self.config,psu_efficiency=.9))
    def test_snapshot_failure_preserves_committed_database(self):
        history=History(self.config);batch=self.batch();history.record(batch)
        with patch("report.MAX_SNAPSHOT_BYTES",1):
            with self.assertRaises(ValueError):Report(history).publish(batch)
        self.assertIsNotNone(history.meta("checkpoint"));history.db.close()
    def test_invalid_configuration_and_nan_readings(self):
        for config in [replace(self.config,psu_efficiency=0),replace(self.config,other_components_watts=float("nan")),
                       replace(self.config,sample_seconds=100),replace(self.config,expected_gpus=3.2),replace(self.config,daily_days=99999)]:
            with self.assertRaises(ValueError):config.validate()
        now=reading(1030,float("nan"),1600)
        self.assertNotIn("estimated-total",{i.id for i in Intervals(self.config).between((reading(),now)).intervals})
    def test_post_restart_resumes_valid_interval_not_replayed(self):
        history=History(self.config);history.record(self.batch())
        previous=Reading.decode(json.loads(history.meta("checkpoint")))
        batch=Intervals(self.config).between((previous,reading(1060,700,2200)))
        history.record(batch)
        value=history.db.execute("SELECT SUM(seconds) FROM buckets WHERE size=60 AND series='0'").fetchone()[0]
        self.assertEqual(value,60)
        history.db.close()

if __name__=="__main__":unittest.main()
