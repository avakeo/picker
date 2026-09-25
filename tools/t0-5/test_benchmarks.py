import importlib.util,tempfile,unittest
from pathlib import Path
spec=importlib.util.spec_from_file_location("bench",Path(__file__).with_name("run_benchmarks.py"));bench=importlib.util.module_from_spec(spec);spec.loader.exec_module(bench)
class Tests(unittest.TestCase):
 def test_recall(self):self.assertEqual(bench.recall([[1,2],[3,4]],[[2,5],[3,6]]),.5)
 def test_normalized(self):
  x=bench.normalized(bench.np.random.default_rng(1),4,8);self.assertTrue(bench.np.allclose(bench.np.linalg.norm(x,axis=1),1.0))
 def test_top_roundtrip(self):
  with tempfile.TemporaryDirectory()as d:
   p=Path(d)/"tops";p.write_text("0,1;2;3\n1,4;5;6\n");self.assertEqual(bench.read_top(p),[[1,2,3],[4,5,6]])
if __name__=="__main__":unittest.main()
