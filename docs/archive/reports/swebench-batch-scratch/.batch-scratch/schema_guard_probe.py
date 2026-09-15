"""Mirror of the swe-harness.sh v5 schema guard — probe it against both parquets."""
import sys

import pyarrow.parquet as pq

cols = set(pq.ParquetFile(sys.argv[1]).schema_arrow.names)
need = {"instance_id", "problem_statement", "base_commit", "repo", "image", "eval_script"}
missing = sorted(need - cols)
print("OK" if not missing else "MISSING:" + ",".join(missing))
