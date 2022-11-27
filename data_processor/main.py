import os
import os.path

import mit
from protos import hrtf_pb2 as proto
import paths

db = mit.compute_hrtf_data()

message = proto.HrtfDataset()

for elev, azs in enumerate(db.azimuths):
    elev_proto = proto.HrtfElevation()
    elev_proto.angle = db.elev_min + db.elev_increment * elev

    for az_num, az in enumerate(azs):
        az_proto = proto.HrtfAzimuth()
        az_proto.angle = az_num * 360 / len(azs)
        az_proto.impulse.extend(az)
        elev_proto.azimuths.extend([az_proto])

    
    message.elevations.extend([elev_proto])

out_dir  = os.path.join(paths.repo_path, "crates", "datasets", "src", "bin_protos")
os.makedirs(out_dir,  exist_ok=True)

out_file = open(os.path.join(out_dir, "mit_kemar.bin"), "wb")
out_file.write(message.SerializeToString())

