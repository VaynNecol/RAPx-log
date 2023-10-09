#!/bin/zsh

os_type=$(uname -s)

echo "\e[4;33mNow building cargo rap for your toolchain.\e[0m"
echo "\e[4;33mPHASE1: Checking operating system.\e[0m"
if [ "$os_type" = "Linux" ]; then
    echo "Detection success: running on \e[1;36mLinux (x86_64-unknown-linux-gnu)\e[0m."
    export HOST_TRIPLE="x86_64-unknown-linux-gnu"
elif [ "$os_type" = "Darwin" ]; then
    echo "Detection success: running on \e[1;36mMacintosh (x86_64-apple-darwin)\e[0m."
    export HOST_TRIPLE="x86_64-apple-darwin"
elif [ "$os_type" = "FreeBSD" ]; then
    echo "Detection success: running on \e[1;36mFreeBSD (x86_64-unknown-linux-gnu)\e[0m."
    export HOST_TRIPLE="x86_64-unknown-linux-gnu"
else
    echo "Detection failed: running unsupported operating system: $os_type."
    echo "\e[4mPress any key to exit...\e[0m"
    read -n 1
    exit 1
fi

echo "\e[4;33mPHASE2: Checking build dependencies \e[1;36mrustup\e[4;33m.\e[0m"
export RUSTUP_SHOW=$(rustup show)
export RAP_DIR=$(dirname "$(readlink -f "$0")")
if [ -z "$(ls ${RAP_DIR}/rust)" ]; then
    echo "Detection failed: directory of rap-rust is empty, please update submodule first."
    echo "\e[4mPress any key to exit...\e[0m"
    read -n 1
    exit 2
else
  if echo "${RUSTUP_SHOW}" | grep -q "rustup"; then
    echo "Detection success: \e[1;36mrustup\e[0m has been linked into \e[1;36mrustup\e[0m toolchain."
  else
    echo "Detection failed: cannot find \e[1;36mrustup\e[0m, please install \e[1;36mrustup\e[0m first."
    echo "\e[4mPress any key to exit...\e[0m"
    read -n 1
    exit 2
  fi
fi

echo "\e[4;31mPHASE3: Building, installing and linking \e[1;36mrap-rust \e[4;31minto \e[1;36mcargo\e[4;31m.\e[0m"
cp -f ./config.toml ./rust/config.toml
cd rust && ./x.py build compiler/rustc -i --stage 2
rustup toolchain link rap-rust build/${HOST_TRIPLE}/stage2

echo "\e[0;32mBuilding success: building, installing and linking \e[1;36mrap-rust \e[0;32mfinished.\e[0m"
echo "\e[1;33mBuild and install all components successfully.\e[0m"
echo "\e[4mPress any key to exit...\e[0m"
read -n 1