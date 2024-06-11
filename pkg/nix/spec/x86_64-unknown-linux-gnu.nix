{ pkgs, target, util }:

{
  inherit target;

  features = with util.features;
    [ storage-mem storage-rocksdb scripting http storage-tikv ]

  buildSpec = with pkgs;
    let crossCompiling = !util.isNative target;
    in {
      depsBuildBuild = [ clang cmake gcc perl protobuf grpc llvm ]
        ++ lib.lists.optional crossCompiling qemu;

      nativeBuildInputs = [ pkg-config ];

      buildInputs = [ openssl ]

      LIBCLANG_PATH = "${llvmPackages.libclang.lib}/lib";

      PROTOC = "${protobuf}/bin/protoc";
      PROTOC_INCLUDE = "${protobuf}/include";

      CARGO_BUILD_TARGET = target;
    };
}
